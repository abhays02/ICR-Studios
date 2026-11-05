//! The NAYEN v1 pipeline as a host-driven state machine.
//!
//! [`Session::start`] returns the first [`HostStep`]; the host performs any
//! inference it is asked for and calls [`Session::feed`] with the results,
//! repeating until `Done`. Because the engine never blocks on a runtime, the
//! same code drives a synchronous native `ort` loop and an async browser loop
//! over `onnxruntime-web`.
//!
//! Stage order matches the reference pipeline: preview, deskew, orient, detect
//! (with one adaptive re-detect), crop, script-route, recognize, then Laya.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::crop::{crop_line, pad_batch, width_batches, LineCrop};
use crate::ctc::{char_positions, collapse};
use crate::det::{clip_to_page, decode, median_height, unscale, LineBox};
use crate::image::{page_to_input, Gray8};
use crate::laya::{self, Extraction, LineReading, RouteDecision, ScriptGroup};
use crate::params::{
    EngineConfig, Profile, ADAPTIVE_MAX_SIDE, ADAPTIVE_MIN_BOXES, ADAPTIVE_MIN_H, ADAPTIVE_TARGET_H,
    DESKEW_COARSE_STEP, DESKEW_MAX, DESKEW_MIN, ORIENT_MIN, ORIENT_SIDE, ORIENT_VERTICAL_CLASS,
};
use crate::skew::{estimate_skew, orient_input, preview, rotate, rotate_quarter};
use crate::tensor::{EngineError, InferRequest, ModelKind, NamedTensor, Tensor};
use crate::vocab::Vocab;

/// Output names the engine asks each model for. Order matters: `feed` reads
/// positionally.
const REC_OUTPUTS: [&str; 3] = ["ids", "conf", "lengths"];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Orient,
    Detect,
    Crop,
    Script,
    Recognize,
    Laya,
    Done,
}

/// Per-stage wall-clock milliseconds, measured inside the engine.
///
/// A stage that waits on the host is timed from the moment the request is
/// emitted to the moment its outputs come back, so these figures include the
/// host's own `session.run`. Nothing has to be added from outside — which is
/// also why nothing can be double-counted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StageTimings {
    pub orient: f64,
    pub deskew: f64,
    pub detect: f64,
    pub crop: f64,
    pub route: f64,
    pub recognize: f64,
    pub laya: f64,
    pub total: f64,
}

/// One batch of inference work the engine still owes itself.
#[derive(Clone, Debug, PartialEq)]
struct Batch {
    model: ModelKind,
    group: ScriptGroup,
    lines: Vec<usize>,
}

/// What the engine asks of the host next.
#[derive(Clone, Debug, PartialEq)]
pub enum HostStep {
    /// Run this model and call [`Session::feed`] with `request.outputs`.
    Infer(InferRequest),
    /// The page is finished.
    Done(Box<PageResult>),
    /// Unrecoverable; carries the reason.
    Failed(EngineError),
}

/// A recognized line.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineResult {
    /// Serialized as `box`; the trailing underscore is only to avoid the
    /// Rust keyword.
    #[serde(rename = "box")]
    pub box_: LineBoxOut,
    /// Empty when confidence was below the gate. Never a guess.
    pub text: String,
    pub conf: f32,
    pub script: ScriptGroup,
    pub needs_review: bool,
    /// Page-x of each character, for overlay highlighting.
    pub char_x: Vec<f32>,
    /// Why the line is flagged, so a reviewer sees the reason and not just a colour.
    pub note: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineBoxOut {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    pub score: f32,
}

impl From<LineBox> for LineBoxOut {
    fn from(b: LineBox) -> Self {
        Self {
            x0: b.x0,
            y0: b.y0,
            x1: b.x1,
            y1: b.y1,
            score: b.score,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageResult {
    pub width: usize,
    pub height: usize,
    pub lines: Vec<LineResult>,
    pub markdown: String,
    pub fields: Option<Extraction>,
    pub timings: StageTimings,
    /// The profile that produced this page. Everything the profile resolves —
    /// detector thresholds, crop geometry — is fixed by this name, so the result
    /// is reproducible from the JSON alone without echoing the whole config.
    pub profile: Profile,
    /// Models the engine actually used, so the UI can show real provenance.
    pub models_used: Vec<ModelKind>,
    /// Degrees counter-clockwise the page was turned to become upright.
    pub rotation: f64,
    /// Quarter turns applied from `orient.onnx`.
    pub quarter_turns: usize,
    /// Non-fatal degradations: a stage skipped because a model was absent, an
    /// adaptive pass that did not help. Surfaced, never silent.
    pub notes: Vec<String>,
}

/// A running page. Create one per page; it holds the working image and all
/// intermediate state between host calls.
pub struct Session {
    cfg: EngineConfig,
    vocabs: BTreeMap<&'static str, Vocab>,
    available: Vec<ModelKind>,
    page: Gray8,
    stage: Stage,
    rotation: f64,
    quarter_turns: usize,
    boxes: Vec<LineBox>,
    /// Detection scale for the current pass, kept so the adaptive pass can beat it.
    det_long_side: usize,
    det_scale: f64,
    /// The adaptive pass runs at most once; without this a still-small second
    /// pass would request a third, and a fourth, until the side cap.
    adaptive_done: bool,
    crops: Vec<Option<LineCrop>>,
    routes: Vec<Option<RouteDecision>>,
    readings: Vec<Option<(String, f32, Vec<f32>)>>,
    queue: Vec<Batch>,
    awaiting: Option<Batch>,
    timings: StageTimings,
    models_used: Vec<ModelKind>,
    notes: Vec<String>,
    /// Start of the stage being timed, read from [`crate::clock`]. `None`
    /// between stages, so a stage that never started cannot contribute a
    /// duration.
    mark: Option<f64>,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("stage", &self.stage)
            .field("boxes", &self.boxes.len())
            .field("page", &(self.page.width, self.page.height))
            .field("queue", &self.queue.len())
            .finish()
    }
}

/// Milliseconds since the mark was set, and clear it. A stage that never called
/// [`start_mark`] contributes nothing rather than an invented duration.
fn elapsed_ms(mark: &mut Option<f64>) -> f64 {
    match mark.take() {
        Some(t) => crate::clock::now_ms() - t,
        None => 0.0,
    }
}

fn start_mark(mark: &mut Option<f64>) {
    *mark = Some(crate::clock::now_ms());
}

impl Session {
    /// `available` is the set of models the host actually loaded. Anything the
    /// engine wants but is not listed is skipped with a recorded note, never
    /// faked.
    pub fn new(
        cfg: EngineConfig,
        rgba: &[u8],
        width: usize,
        height: usize,
        vocabs: BTreeMap<&'static str, Vocab>,
        available: Vec<ModelKind>,
    ) -> Result<Self, EngineError> {
        if vocabs.get("base").is_none() {
            return Err(EngineError::Decode(
                "the base recognizer vocabulary is required to decode any text".into(),
            ));
        }
        let gray = Gray8::from_rgb(rgba, width, height, 4)?;
        let det_long_side = cfg.detect.long_side;
        Ok(Self {
            cfg,
            vocabs,
            available,
            page: gray,
            stage: Stage::Orient,
            rotation: 0.0,
            quarter_turns: 0,
            boxes: Vec::new(),
            det_long_side,
            det_scale: 1.0,
            adaptive_done: false,
            crops: Vec::new(),
            routes: Vec::new(),
            readings: Vec::new(),
            queue: Vec::new(),
            awaiting: None,
            timings: StageTimings::default(),
            models_used: Vec::new(),
            notes: Vec::new(),
                        mark: None,
        })
    }

    fn can(&self, m: ModelKind) -> bool {
        self.available.contains(&m)
    }

    fn note(&mut self, s: impl Into<String>) {
        self.notes.push(s.into());
    }

    fn use_model(&mut self, m: ModelKind) {
        if !self.models_used.contains(&m) {
            self.models_used.push(m);
        }
    }

    /// Cap the working page so a 600dpi scan cannot exhaust a WASM heap.
    fn cap_page(&mut self) {
        let longest = self.page.width.max(self.page.height);
        if longest > self.cfg.max_page_side {
            match preview(&self.page, self.cfg.max_page_side) {
                p if p.width > 0 => {
                    self.note(format!(
                        "page downscaled from {}px to {}px to fit the configured maximum",
                        longest, self.cfg.max_page_side
                    ));
                    self.page = p;
                }
                _ => {}
            }
        }
    }

    /// Begin the page. Pure stages (preview, deskew) run immediately; the first
    /// model call is returned to the host.
    pub fn start(&mut self) -> HostStep {
        self.cap_page();

        if self.cfg.deskew {
                        start_mark(&mut self.mark);
            let a = estimate_skew(
                &self.page,
                self.cfg.skew_side,
                DESKEW_MAX,
                DESKEW_COARSE_STEP,
            );
            self.timings.deskew += elapsed_ms(&mut self.mark);
            if a.abs() >= DESKEW_MIN {
                self.page = rotate(&self.page, a);
                self.rotation = a;
            }
        }

        if self.cfg.orient && self.can(ModelKind::Orient) {
            self.request_orient()
        } else {
            if self.cfg.orient {
                self.note("orient.onnx not loaded; quarter-turn correction skipped");
            }
            self.begin_detect()
        }
    }

    fn request_orient(&mut self) -> HostStep {
                start_mark(&mut self.mark);
        let data = orient_input(&self.page, ORIENT_SIDE);
        self.stage = Stage::Orient;
        self.awaiting = Some(Batch {
            model: ModelKind::Orient,
            group: ScriptGroup::Base,
            lines: Vec::new(),
        });
        HostStep::Infer(InferRequest {
            model: ModelKind::Orient,
            inputs: vec![NamedTensor::new(
                "x",
                Tensor::f32(vec![1, 1, ORIENT_SIDE as i64, ORIENT_SIDE as i64], data),
            )],
            outputs: vec!["p".into()],
        })
    }

    fn begin_detect(&mut self) -> HostStep {
        self.det_long_side = self.cfg.detect.long_side;
        self.request_detect(self.det_long_side)
    }

    fn request_detect(&mut self, long_side: usize) -> HostStep {
                start_mark(&mut self.mark);
        let (ink, scale) = match page_to_input(&self.page, long_side) {
            Ok(v) => v,
            Err(e) => return HostStep::Failed(EngineError::Image(e.to_string())),
        };
        self.det_scale = scale;
        self.stage = Stage::Detect;
        self.awaiting = Some(Batch {
            model: ModelKind::Det,
            group: ScriptGroup::Base,
            lines: Vec::new(),
        });
        HostStep::Infer(InferRequest {
            model: ModelKind::Det,
            inputs: vec![NamedTensor::new(
                "x",
                Tensor::f32(
                    vec![1, 1, ink.height as i64, ink.width as i64],
                    ink.data,
                ),
            )],
            outputs: vec!["out".into()],
        })
    }

    /// Handle a detector output: decode, then decide whether the adaptive pass
    /// is warranted. Returns `Some(step)` when another detection is needed.
    fn on_detect(&mut self, out: &Tensor) -> Option<HostStep> {
        self.timings.detect += elapsed_ms(&mut self.mark);
        self.use_model(ModelKind::Det);

        // Output is [1, 3, h, w]; h and w are half the padded input.
        let shape = &out.shape;
        let (ch, h, w) = if shape.len() == 4 {
            (shape[1], shape[2], shape[3])
        } else {
            return Some(HostStep::Failed(EngineError::ShapeMismatch {
                expected: "[1,3,h,w]".into(),
                got: format!("{shape:?}"),
            }));
        };
        if ch != 3 {
            self.note(format!(
                "det.onnx returned {ch} channels; the picture channel is not used by NAYEN v1"
            ));
        }
        let data = match out.data.as_f32() {
            Some(d) => d,
            None => {
                return Some(HostStep::Failed(EngineError::ShapeMismatch {
                    expected: "float32 detector output".into(),
                    got: format!("{:?}", out.dtype()),
                }))
            }
        };

        let mut boxes = decode(data, h as usize, w as usize, &self.cfg.detect);
        unscale(&mut boxes, self.det_scale);
        clip_to_page(&mut boxes, self.page.width, self.page.height);

        // Adaptive re-detection: small text at the base scale produces short
        // boxes, and a larger input resolves them. One pass only.
        if self.cfg.detect.adaptive
            && !self.adaptive_done
            && boxes.len() >= ADAPTIVE_MIN_BOXES
        {
            let med = median_height(&boxes);
            // median_height is in page pixels; the rule is stated in input
            // pixels, so convert with the scale actually used.
            let med_input = med * self.det_scale as f32;
            if med_input > 0.0 && med_input < ADAPTIVE_MIN_H {
                let ls = ((self.det_long_side as f64 * ADAPTIVE_TARGET_H as f64 / med_input as f64)
                    .min(ADAPTIVE_MAX_SIDE as f64) as usize)
                    / 32
                    * 32;
                if ls > self.det_long_side {
                    // Keep the current result; if the larger pass finds fewer
                    // lines it is discarded rather than trusted.
                    self.boxes = boxes;
                    self.adaptive_done = true;
                    self.det_long_side = ls;
                    self.note(format!("small text (median {med_input:.1}px); re-detecting at {ls}px"));
                    return Some(self.request_detect(ls));
                }
            }
        }

        // If this was an adaptive pass, only take it if it found more lines.
        if !self.boxes.is_empty() {
            if boxes.len() > self.boxes.len() {
                self.boxes = boxes;
            } else {
                self.note(format!(
                    "re-detection at {}px found {} lines vs {}; keeping the first pass",
                    self.det_long_side,
                    boxes.len(),
                    self.boxes.len()
                ));
            }
        } else {
            self.boxes = boxes;
        }

        None
    }

    /// Build recognizer crops for every box. Lines that fail to crop are recorded
    /// and skipped, so one bad box cannot abort a page.
    fn build_crops(&mut self) -> Result<(), EngineError> {
                start_mark(&mut self.mark);
        let (boxes, page, crop_cfg) = (self.boxes.clone(), self.page.clone(), self.cfg.crop);
        self.crops = Vec::with_capacity(boxes.len());
        for (i, b) in boxes.iter().enumerate() {
            match crop_line(&page, b, &crop_cfg) {
                Ok(c) => self.crops.push(Some(c)),
                Err(e) => {
                    self.note(format!("line {i} could not be cropped: {e}"));
                    self.crops.push(None);
                }
            }
        }
        self.timings.crop += elapsed_ms(&mut self.mark);
        Ok(())
    }

    /// Queue script-classifier batches, when there is anything to route between.
    fn begin_script(&mut self) -> HostStep {
        let groups_loaded = [
            ModelKind::RecCjk,
            ModelKind::RecRtl,
            ModelKind::RecWorld,
        ]
        .iter()
        .filter(|m| self.can(**m))
        .count();

        if !self.cfg.route_scripts || !self.can(ModelKind::Script) || groups_loaded == 0 {
            if self.cfg.route_scripts && groups_loaded == 0 {
                self.note("no group recognizers loaded; every line is read by rec.onnx");
            } else if self.cfg.route_scripts {
                self.note("script.onnx not loaded; every line is read by rec.onnx");
            }
            return self.begin_recognize();
        }

        let lines: Vec<usize> = (0..self.crops.len()).filter(|&i| self.crops[i].is_some()).collect();
        if lines.is_empty() {
            return self.begin_recognize();
        }
        self.routes = vec![None; self.boxes.len()];
        let widths: Vec<usize> = lines
            .iter()
            .map(|&i| self.crops[i].as_ref().map(|c| c.width).unwrap_or(64))
            .collect();
        self.queue = width_batches(&widths)
            .into_iter()
            .map(|part| Batch {
                model: ModelKind::Script,
                group: ScriptGroup::Base,
                lines: part.into_iter().map(|k| lines[k]).collect(),
            })
            .collect();
        self.stage = Stage::Script;
        self.next_request()
    }

    /// Build a script-classifier batch: 32px crops, no window.
    fn script_tensors(&self, lines: &[usize]) -> Option<(NamedTensor, NamedTensor)> {
        let p = crate::params::CropParams::script_classifier();
        let mut crops: Vec<LineCrop> = Vec::with_capacity(lines.len());
        for &i in lines {
            crops.push(crop_line(&self.page, &self.boxes[i], &p).ok()?);
        }
        let refs: Vec<&LineCrop> = crops.iter().collect();
        pad_batch(&refs)
    }

    fn begin_recognize(&mut self) -> HostStep {
        let lines: Vec<usize> = (0..self.crops.len()).filter(|&i| self.crops[i].is_some()).collect();
        if lines.is_empty() {
            self.stage = Stage::Laya;
            return self.finish();
        }

        // A line goes to its group recognizer only when the classifier was sure
        // AND that recognizer is loaded. Everything else is read by base; when
        // the classifier was unsure, both read it and the higher confidence wins.
        let mut by_model: BTreeMap<(ModelKind, ScriptGroup), Vec<usize>> = BTreeMap::new();
        for &i in &lines {
            let route = self.routes.get(i).copied().flatten();
            match route {
                Some(r) if r.exclusive && r.group != ScriptGroup::Base => {
                    let m = match r.group {
                        ScriptGroup::Cjk => ModelKind::RecCjk,
                        ScriptGroup::Rtl => ModelKind::RecRtl,
                        ScriptGroup::Other => ModelKind::RecWorld,
                        ScriptGroup::Base => ModelKind::Rec,
                    };
                    if self.can(m) {
                        by_model.entry((m, r.group)).or_default().push(i);
                        continue;
                    }
                    self.note(format!(
                        "line {i} routed to {} but {} is not loaded; read by rec.onnx instead",
                        r.group.as_str(),
                        m.filename()
                    ));
                }
                _ => {}
            }
            by_model
                .entry((ModelKind::Rec, ScriptGroup::Base))
                .or_default()
                .push(i);
        }

        self.queue = Vec::new();
        for ((model, group), mut idx) in by_model {
            // width_batches expects widths in the same order as the indices it returns.
            idx.sort_by_key(|&i| self.crops[i].as_ref().map(|c| c.width).unwrap_or(0));
            let widths: Vec<usize> = idx
                .iter()
                .map(|&i| self.crops[i].as_ref().map(|c| c.width).unwrap_or(64))
                .collect();
            for part in width_batches(&widths) {
                self.queue.push(Batch {
                    model,
                    group,
                    lines: part.into_iter().map(|k| idx[k]).collect(),
                });
            }
        }
        self.readings = vec![None; self.boxes.len()];
        self.stage = Stage::Recognize;
        self.next_request()
    }

    /// Emit the next queued batch, or advance the stage when the queue is empty.
    fn next_request(&mut self) -> HostStep {
        if self.queue.is_empty() {
            return match self.stage {
                Stage::Script => self.begin_recognize(),
                Stage::Recognize => {
                    self.stage = Stage::Laya;
                    self.finish()
                }
                _ => self.finish(),
            };
        }
        let batch = self.queue.remove(0);
        let req = if batch.model == ModelKind::Script {
            match self.script_tensors(&batch.lines) {
                Some((x, widths)) => InferRequest {
                    model: ModelKind::Script,
                    inputs: vec![x, widths],
                    outputs: vec!["p".into()],
                },
                None => {
                    self.note("a script batch could not be cropped; those lines default to base");
                    return self.next_request();
                }
            }
        } else {
            let crops: Vec<&LineCrop> = batch
                .lines
                .iter()
                .filter_map(|&i| self.crops[i].as_ref())
                .collect();
            match pad_batch(&crops) {
                Some((x, widths)) => InferRequest {
                    model: batch.model,
                    inputs: vec![x, widths],
                    outputs: REC_OUTPUTS.iter().map(|s| s.to_string()).collect(),
                },
                None => return self.next_request(),
            }
        };
        self.use_model(batch.model);
        start_mark(&mut self.mark);
        self.awaiting = Some(batch);
        HostStep::Infer(req)
    }

    /// Feed the outputs of the outstanding request back in.
    pub fn feed(&mut self, outputs: Vec<Tensor>) -> HostStep {
        let Some(batch) = self.awaiting.take() else {
            return HostStep::Failed(EngineError::Host(
                "feed() called with no outstanding inference request".into(),
            ));
        };
        // Det is charged by on_detect, which also covers the pure decode work
        // that follows the host round-trip — so its mark must survive this block
        // rather than be consumed and discarded here.
        if batch.model != ModelKind::Det {
            let ms = elapsed_ms(&mut self.mark);
            match batch.model {
                ModelKind::Orient => self.timings.orient += ms,
                ModelKind::Script => self.timings.route += ms,
                _ => self.timings.recognize += ms,
            }
        }

        // Detection is handled apart from the others because it may ask for a
        // second pass at a larger scale, which is another host round-trip.
        if batch.model == ModelKind::Det {
            let Some(out) = outputs.first() else {
                return HostStep::Failed(EngineError::ShapeMismatch {
                    expected: "one [1,3,h,w] detector output".into(),
                    got: "none".into(),
                });
            };
            return match self.on_detect(out) {
                Some(step) => step,
                None => self.after_detect(),
            };
        }

        let result = match batch.model {
            ModelKind::Orient => self.on_orient(&outputs),
            ModelKind::Script => self.on_script(&batch, &outputs),
            _ => self.on_recognize(&batch, &outputs),
        };
        if let Err(e) = result {
            return HostStep::Failed(e);
        }

        match self.stage {
            Stage::Orient => self.begin_detect(),
            Stage::Script | Stage::Recognize => self.next_request(),
            _ => self.finish(),
        }
    }

    fn on_orient(&mut self, outputs: &[Tensor]) -> Result<(), EngineError> {
        let p = outputs
            .first()
            .and_then(|t| t.data.as_f32())
            .ok_or_else(|| EngineError::ShapeMismatch {
                expected: "float32 [B,5] orientation probabilities".into(),
                got: "missing or wrong dtype".into(),
            })?;
        self.use_model(ModelKind::Orient);
        if p.len() < 4 {
            self.note("orient.onnx returned fewer than 4 classes; no turn applied");
            return Ok(());
        }
        let mut k = 0usize;
        for (i, v) in p.iter().enumerate().take(5) {
            if *v > p[k] {
                k = i;
            }
        }
        if k == ORIENT_VERTICAL_CLASS {
            self.note(
                "page is set in vertical columns; NAYEN v1 reads horizontal lines only, \
                 so results may be incomplete"
                    .to_string(),
            );
            return Ok(());
        }
        if p[k] > ORIENT_MIN && k != 0 {
            self.page = rotate_quarter(&self.page, k);
            self.quarter_turns = k;
            self.note(format!("page turned {k} quarter turn(s) to upright (p={:.3})", p[k]));
        }
        Ok(())
    }

    fn on_script(&mut self, batch: &Batch, outputs: &[Tensor]) -> Result<(), EngineError> {
        let p = outputs
            .first()
            .and_then(|t| t.data.as_f32())
            .ok_or_else(|| EngineError::ShapeMismatch {
                expected: "float32 [B,4] script probabilities".into(),
                got: "missing or wrong dtype".into(),
            })?;
        self.use_model(ModelKind::Script);
        let classes = if outputs[0].shape.len() >= 2 {
            outputs[0].shape[1].max(1) as usize
        } else {
            4
        };
        for (k, &i) in batch.lines.iter().enumerate() {
            let start = k * classes;
            if start + classes > p.len() {
                break;
            }
            self.routes[i] = Some(laya::route(&p[start..start + classes]));
        }
        Ok(())
    }

    fn on_recognize(&mut self, batch: &Batch, outputs: &[Tensor]) -> Result<(), EngineError> {
        if outputs.len() < 3 {
            return Err(EngineError::ShapeMismatch {
                expected: "ids, conf, lengths".into(),
                got: format!("{} outputs", outputs.len()),
            });
        }
        self.use_model(batch.model);
        let group_key = batch.group.as_str();
        // A group recognizer's ids mean nothing in the base vocabulary.
        let vocab = self
            .vocabs
            .get(group_key)
            .or_else(|| self.vocabs.get("base"))
            .ok_or_else(|| EngineError::Decode(format!("no vocabulary for group {group_key}")))?
            .clone();

        let timesteps = outputs[0].shape.last().copied().unwrap_or(0).max(0) as usize;
        let lengths = outputs[2]
            .data
            .as_i64()
            .ok_or_else(|| EngineError::ShapeMismatch {
                expected: "int64 lengths".into(),
                got: format!("{:?}", outputs[2].dtype()),
            })?;
        // Padded tensor width, needed to map timesteps back to page pixels.
        let padded_width = batch
            .lines
            .iter()
            .map(|&i| self.crops[i].as_ref().map(|c| c.width).unwrap_or(64))
            .max()
            .unwrap_or(64);
        let padded_width = padded_width.div_ceil(8) * 8;

        for (k, &i) in batch.lines.iter().enumerate() {
            let n = lengths.get(k).copied().unwrap_or(0).max(0) as usize;
            if n == 0 {
                continue;
            }
            let ids_row = match outputs[0].row_i64(k, timesteps) {
                Some(r) => r,
                None => continue,
            };
            let conf_row = match outputs[1].row_f32(k, timesteps) {
                Some(r) => r,
                None => continue,
            };
            let n = n.min(ids_row.len()).min(conf_row.len());
            let reading = collapse(&ids_row[..n], &conf_row[..n], vocab.blank);
            let text = vocab.decode(&reading.ids);
            let crop_w = self.crops[i].as_ref().map(|c| c.width).unwrap_or(padded_width);
            let b = self.boxes[i];
            let xs = char_positions(
                &reading.steps,
                timesteps,
                padded_width,
                crop_w,
                b.x0,
                b.x1,
                (b.y1 - b.y0).max(1.0) as f64,
                self.cfg.crop.pad_x,
            );

            // Confidence gate: below it the line keeps its box but yields no
            // text. On empty fields and rules the recognizer reads "-", "I" or
            // "1" with low confidence, and those look exactly like real data.
            let emitted = if reading.conf >= self.cfg.min_conf {
                text
            } else {
                String::new()
            };

            // The reference keeps the more confident reading when two
            // recognizers both read a line.
            let better = match &self.readings[i] {
                Some((_, c, _)) => reading.conf > *c,
                None => true,
            };
            if better {
                self.readings[i] = Some((emitted, reading.conf, xs));
            }
        }
        Ok(())
    }

    /// Assemble lines, run Laya, and build the reading-ordered markdown.
    fn finish(&mut self) -> HostStep {
                start_mark(&mut self.mark);

        let mut lines: Vec<LineResult> = Vec::with_capacity(self.boxes.len());

        for (i, b) in self.boxes.iter().enumerate() {
            let (text, conf, char_x) = self
                .readings
                .get(i)
                .and_then(|r| r.clone())
                .unwrap_or_else(|| (String::new(), 0.0, Vec::new()));
            let group = self
                .routes
                .get(i)
                .and_then(|r| *r)
                .map(|r| r.group)
                .unwrap_or(ScriptGroup::Base);

            let band = laya::ConfidenceBand::of(conf);
            let note = match band {
                laya::ConfidenceBand::Rejected if !text.is_empty() => Some(format!(
                    "confidence {conf:.2} is below the {:.2} gate; text suppressed",
                    self.cfg.min_conf
                )),
                laya::ConfidenceBand::Rejected => Some("no confident reading".into()),
                laya::ConfidenceBand::Review => Some(format!(
                    "confidence {conf:.2} is readable but below 0.90; worth a glance"
                )),
                laya::ConfidenceBand::Verified => None,
            };
            let needs_review = conf < self.cfg.min_conf
                || conf < 0.9
                || group != ScriptGroup::Base;

            lines.push(LineResult {
                box_: (*b).into(),
                text: if conf >= self.cfg.min_conf {
                    text.clone()
                } else {
                    String::new()
                },
                conf,
                script: group,
                needs_review,
                char_x,
                note,
            });
        }

        // Laya reads the owned lines, so the borrow outlives the extraction.
        let readings: Vec<LineReading> = lines
            .iter()
            .map(|l| LineReading {
                text: &l.text,
                conf: l.conf,
                group: l.script,
                y0: l.box_.y0,
                y1: l.box_.y1,
                x0: l.box_.x0,
                x1: l.box_.x1,
            })
            .collect();

        let fields = if self.cfg.laya && !readings.is_empty() {
            let e = laya::extract(&readings, self.cfg.min_conf);
            for a in &e.anomalies {
                if matches!(a.severity, laya::Severity::Fault) {
                    self.note(format!("{}: {}", a.code, a.message));
                }
            }
            Some(e)
        } else {
            None
        };

        // Reading order. `order.onnx` is not wired up in v1, so this is a
        // top-to-bottom, left-to-right XY sort with row banding: correct for
        // single- and multi-column forms with a consistent baseline, and
        // explicitly not a layout model.
        order_lines(&mut lines);
        let markdown = to_markdown(&lines);

        self.timings.laya += elapsed_ms(&mut self.mark);
        self.timings.total = self.timings.orient
            + self.timings.deskew
            + self.timings.detect
            + self.timings.crop
            + self.timings.route
            + self.timings.recognize
            + self.timings.laya;

        self.stage = Stage::Done;
        HostStep::Done(Box::new(PageResult {
            width: self.page.width,
            height: self.page.height,
            lines,
            markdown,
            fields,
            timings: self.timings,
            profile: self.cfg.profile,
            models_used: self.models_used.clone(),
            rotation: self.rotation,
            quarter_turns: self.quarter_turns,
            notes: self.notes.clone(),
        }))
    }

    /// Advance past detection into cropping.
    fn after_detect(&mut self) -> HostStep {
        if self.boxes.is_empty() {
            self.note("no text lines detected on this page");
            self.stage = Stage::Laya;
            return self.finish();
        }
        if let Err(e) = self.build_crops() {
            return HostStep::Failed(e);
        }
        self.stage = Stage::Crop;
        self.begin_script()
    }

    pub fn stage(&self) -> Stage {
        self.stage
    }

    pub fn notes(&self) -> &[String] {
        &self.notes
    }

    pub fn line_count(&self) -> usize {
        self.boxes.len()
    }
}

/// Row-band then left-to-right ordering.
///
/// Lines are grouped into bands by vertical overlap rather than by y alone, so
/// two cells of one table row that differ by a pixel of detected top edge stay
/// on the same line.
fn order_lines(lines: &mut [LineResult]) {
    lines.sort_by(|a, b| {
        let ay = (a.box_.y0 + a.box_.y1) / 2.0;
        let by = (b.box_.y0 + b.box_.y1) / 2.0;
        let ah = (a.box_.y1 - a.box_.y0).max(1.0);
        let bh = (b.box_.y1 - b.box_.y0).max(1.0);
        // Same band when the centres are within half a line height of each other.
        let band = (ah.min(bh) * 0.6).max(2.0);
        if (ay - by).abs() <= band {
            a.box_
                .x0
                .partial_cmp(&b.box_.x0)
                .unwrap_or(std::cmp::Ordering::Equal)
        } else {
            ay.partial_cmp(&by).unwrap_or(std::cmp::Ordering::Equal)
        }
    });
}

/// Lines to Markdown. A line that was detected but not read stays as a blank
/// entry: its box is real layout evidence even though its text is not known.
fn to_markdown(lines: &[LineResult]) -> String {
    let mut out = String::new();
    for l in lines {
        let t = l.text.trim();
        if t.is_empty() {
            out.push_str("<!-- line detected, no confident reading -->\n");
        } else {
            out.push_str(t);
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::Profile;
    use crate::vocab::Vocab;

    /// A minimal executor that returns canned outputs, so the state machine can
    /// be driven without any ONNX runtime.
    struct FakeHost {
        det_h: usize,
        det_w: usize,
        bands: Vec<(usize, usize)>,
        script_probs: Option<Vec<f32>>,
        rec_text: Vec<i64>,
    }

    impl FakeHost {
        fn new() -> Self {
            Self {
                det_h: 16,
                det_w: 32,
                bands: vec![(6, 9)],
                script_probs: None,
                rec_text: vec![1, 2, 3],
            }
        }

        fn det_output(&self) -> Tensor {
            let mut out = vec![0f32; 3 * self.det_h * self.det_w];
            let plane = self.det_h * self.det_w;
            for &(y0, y1) in &self.bands {
                for y in y0..y1.min(self.det_h) {
                    for x in 4..20.min(self.det_w) {
                        let i = y * self.det_w + x;
                        out[i] = 6.0;
                        out[plane + i] = ((y - y0) as f32 + 0.5) * 2.0;
                        out[2 * plane + i] = ((y1 - 1 - y) as f32 + 0.5) * 2.0;
                    }
                }
            }
            Tensor::f32(vec![1, 3, self.det_h as i64, self.det_w as i64], out)
        }
    }

    fn vocabs() -> BTreeMap<&'static str, Vocab> {
        let mut m = BTreeMap::new();
        m.insert(
            "base",
            Vocab::from_json_str(r#"{"blank":0,"chars":["a","b","c"," "]}"#).unwrap(),
        );
        m
    }

    /// Test page size. Deliberately larger than `det_long_side` (1024) so the
    /// page is scaled *down*: at 400x200 the scale is 2.56, decoded boxes come
    /// back under 2px once unscaled, and `clip_to_page` correctly discards them.
    const PAGE_W: usize = 1600;
    const PAGE_H: usize = 900;

    /// An RGBA page with one dark bar where the detector will look.
    fn test_page(w: usize, h: usize) -> Vec<u8> {
        let mut rgba = vec![255u8; w * h * 4];
        for y in (h / 3)..(h / 3 + 20) {
            for x in 10..(w - 10) {
                for c in 0..3 {
                    rgba[(y * w + x) * 4 + c] = 0;
                }
            }
        }
        rgba
    }

    /// Deskew off and a small search thumbnail: it is ~90 rotations of a
    /// thumbnail, which would dominate the unit-test runtime without changing
    /// what these tests assert. The deskew stage is covered in `skew`'s own tests.
    fn fast_cfg(profile: Profile) -> EngineConfig {
        EngineConfig {
            deskew: false,
            skew_side: 128,
            ..EngineConfig::from_profile(profile)
        }
    }

    /// Drive a session to completion with canned model outputs.
    fn drive(cfg: EngineConfig, host: &FakeHost, available: Vec<ModelKind>) -> PageResult {
        let rgba = test_page(PAGE_W, PAGE_H);
        let mut s = Session::new(cfg, &rgba, PAGE_W, PAGE_H, vocabs(), available).unwrap();
        let mut step = s.start();
        let mut guard = 0;
        loop {
            guard += 1;
            assert!(guard < 64, "state machine did not terminate");
            match step {
                HostStep::Infer(req) => {
                    let outs = match req.model {
                        ModelKind::Orient => {
                            vec![Tensor::f32(vec![1, 5], vec![0.99, 0.0, 0.0, 0.0, 0.01])]
                        }
                        ModelKind::Det => vec![host.det_output()],
                        ModelKind::Script => vec![Tensor::f32(
                            vec![req.inputs[0].tensor.shape[0], 4],
                            host.script_probs
                                .clone()
                                .unwrap_or_else(|| vec![0.98, 0.01, 0.005, 0.005]),
                        )],
                        _ => {
                            let b = req.inputs[0].tensor.shape[0] as usize;
                            let t = 8i64;
                            let mut ids = vec![0i64; b * t as usize];
                            let conf = vec![0.95f32; b * t as usize];
                            let mut lengths = vec![0i64; b];
                            for row in 0..b {
                                for (k, &id) in host.rec_text.iter().enumerate() {
                                    ids[row * t as usize + k] = id;
                                }
                                lengths[row] = host.rec_text.len() as i64;
                            }
                            vec![
                                Tensor::i64(vec![b as i64, t], ids),
                                Tensor::f32(vec![b as i64, t], conf),
                                Tensor::i64(vec![b as i64], lengths),
                            ]
                        }
                    };
                    step = s.feed(outs);
                }
                HostStep::Done(r) => return *r,
                HostStep::Failed(e) => panic!("engine failed: {e}"),
            }
        }
    }

    #[test]
    fn base_vocabulary_is_required() {
        let rgba = test_page(PAGE_W, PAGE_H);
        let e = Session::new(
            fast_cfg(Profile::Indra),
            &rgba,
            PAGE_W,
            PAGE_H,
            BTreeMap::new(),
            vec![ModelKind::Det, ModelKind::Rec],
        );
        assert!(matches!(e, Err(EngineError::Decode(_))));
    }

    #[test]
    fn core_only_host_runs_the_full_pipeline() {
        // Only det + rec loaded: orient and script must be skipped with notes,
        // and the page must still produce text.
        let host = FakeHost::new();
        let r = drive(
            fast_cfg(Profile::Indra),
            &host,
            vec![ModelKind::Det, ModelKind::Rec],
        );
        assert!(!r.lines.is_empty(), "detector found a band, so there is a line");
        assert_eq!(r.lines[0].text, "abc", "ids 1,2,3 decode to a,b,c");
        assert!(r.lines[0].conf > 0.9);
        assert_eq!(r.models_used, vec![ModelKind::Det, ModelKind::Rec]);
        assert!(r
            .notes
            .iter()
            .any(|n| n.contains("orient.onnx not loaded")));
        assert!(!r.markdown.is_empty());
    }

    #[test]
    fn full_host_adds_orientation_and_routing() {
        let host = FakeHost {
            script_probs: Some(vec![0.98, 0.01, 0.005, 0.005]),
            ..FakeHost::new()
        };
        let r = drive(
            fast_cfg(Profile::Indra),
            &host,
            vec![ModelKind::Orient, ModelKind::Det, ModelKind::Script, ModelKind::Rec],
        );
        assert!(r.models_used.contains(&ModelKind::Orient));
        // p[0] = 0.99 for upright, so no turn is applied.
        assert_eq!(r.quarter_turns, 0);
        assert_eq!(r.lines[0].script, ScriptGroup::Base);
    }

    #[test]
    fn low_confidence_reading_yields_a_box_but_no_text() {
        let host = FakeHost::new();
        let rgba = test_page(PAGE_W, PAGE_H);
        let mut s = Session::new(
            fast_cfg(Profile::Indra),
            &rgba,
            PAGE_W,
            PAGE_H,
            vocabs(),
            vec![ModelKind::Det, ModelKind::Rec],
        )
        .unwrap();
        let mut step = s.start();
        let mut guard = 0;
        let result = loop {
            guard += 1;
            assert!(guard < 64);
            match step {
                HostStep::Infer(req) => {
                    let outs = match req.model {
                        ModelKind::Det => vec![host.det_output()],
                        _ => {
                            let b = req.inputs[0].tensor.shape[0] as usize;
                            let t = 8i64;
                            vec![
                                Tensor::i64(vec![b as i64, t], vec![1i64; b * t as usize]),
                                // Confidence far below the 0.5 gate.
                                Tensor::f32(vec![b as i64, t], vec![0.2f32; b * t as usize]),
                                Tensor::i64(vec![b as i64], vec![3; b]),
                            ]
                        }
                    };
                    step = s.feed(outs);
                }
                HostStep::Done(r) => break *r,
                HostStep::Failed(e) => panic!("{e}"),
            }
        };
        assert_eq!(result.lines.len(), 1, "the box is real layout evidence");
        assert_eq!(result.lines[0].text, "", "but no text is emitted");
        assert!(result.lines[0].needs_review);
        assert!(result.lines[0].note.is_some());
        assert!(result.markdown.contains("no confident reading"));
    }

    #[test]
    fn blank_page_reports_no_lines_without_fabricating() {
        let host = FakeHost {
            bands: vec![],
            ..FakeHost::new()
        };
        let r = drive(
            fast_cfg(Profile::Indra),
            &host,
            vec![ModelKind::Det, ModelKind::Rec],
        );
        assert!(r.lines.is_empty());
        assert!(r.notes.iter().any(|n| n.contains("no text lines detected")));
        assert_eq!(r.markdown, "");
    }

    #[test]
    fn deskew_can_be_disabled_and_page_is_capped() {
        let cfg = EngineConfig {
            deskew: false,
            max_page_side: 150,
            ..fast_cfg(Profile::Indra)
        };
        let host = FakeHost::new();
        let r = drive(cfg, &host, vec![ModelKind::Det, ModelKind::Rec]);
        assert_eq!(r.rotation, 0.0);
        assert!(
            r.width.max(r.height) <= 150,
            "page must be capped to max_page_side, got {}x{}",
            r.width,
            r.height
        );
        assert!(r.notes.iter().any(|n| n.contains("downscaled")));
    }

    #[test]
    fn markdown_preserves_reading_order() {
        // Three-row bands: decoded height is 2 * (y1 - y0) input pixels, which
        // at this page's scale stays well above clip_to_page's 2px floor.
        let host = FakeHost {
            bands: vec![(3, 6), (11, 14)],
            ..FakeHost::new()
        };
        let r = drive(
            fast_cfg(Profile::Indra),
            &host,
            vec![ModelKind::Det, ModelKind::Rec],
        );
        assert_eq!(r.lines.len(), 2);
        let ys: Vec<f32> = r.lines.iter().map(|l| l.box_.y0).collect();
        assert!(ys[0] <= ys[1], "top band first: {ys:?}");
        assert_eq!(r.markdown.lines().count(), 2);
    }

    #[test]
    fn feed_without_a_request_is_an_error() {
        let rgba = test_page(200, 100);
        let mut s = Session::new(
            EngineConfig::indra(),
            &rgba,
            200,
            100,
            vocabs(),
            vec![ModelKind::Det, ModelKind::Rec],
        )
        .unwrap();
        let step = s.feed(vec![]);
        assert!(matches!(step, HostStep::Failed(EngineError::Host(_))));
    }

    #[test]
    fn a_fresh_session_has_measured_nothing() {
        let rgba = test_page(PAGE_W, PAGE_H);
        let mut s = Session::new(
            fast_cfg(Profile::Indra),
            &rgba,
            PAGE_W,
            PAGE_H,
            vocabs(),
            vec![ModelKind::Det, ModelKind::Rec],
        )
        .unwrap();
        // Timings are only filled in as a page runs; a fresh session has
        // measured nothing and must claim nothing.
        assert_eq!(s.timings, StageTimings::default());
    }
}
