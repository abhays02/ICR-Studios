//! `nayen` — the native driver for the NAYEN v1 engine.
//!
//! Runs a page end to end through ONNX Runtime and prints what the engine
//! actually did, including every stage it skipped and why.

mod ort_exec;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use image::GenericImageView;
use nayen_core::pipeline::{HostStep, PageResult, Session};
use nayen_core::tensor::{Executor, ModelKind};
use nayen_core::vocab::Meta;
use nayen_core::{EngineConfig, Profile, Vocab};
use ort_exec::OrtExecutor;

/// Every model the engine can use. `--core` and `--fast` select subsets.
const ALL_MODELS: [ModelKind; 8] = [
    ModelKind::Orient,
    ModelKind::Det,
    ModelKind::Script,
    ModelKind::Rec,
    ModelKind::RecWorld,
    ModelKind::RecCjk,
    ModelKind::RecRtl,
    ModelKind::Order,
];

#[derive(Parser)]
#[command(name = "nayen", version, about = "NAYEN v1 - native Indra + Laya ICR engine")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Recognize one image and print the result.
    Run {
        /// PNG or JPEG page.
        image: PathBuf,
        #[arg(long, value_enum, default_value_t = ProfileArg::Indra)]
        profile: ProfileArg,
        #[arg(long, default_value = "var/models/indra-ocr")]
        models: PathBuf,
        /// det|rec only (fastest), or all (full pipeline).
        #[arg(long, value_enum, default_value_t = ModelSet::Fast)]
        set: ModelSet,
        /// Emit the full PageResult as JSON instead of readable text.
        #[arg(long)]
        json: bool,
        #[arg(long, default_value_t = 0)]
        threads: usize,
        /// Skip the deskew search.
        #[arg(long)]
        no_deskew: bool,
    },
    /// Print each model's real input/output tensor contracts.
    Inspect {
        #[arg(long, default_value = "var/models/indra-ocr")]
        models: PathBuf,
    },
    /// Time each stage over repeated runs of one image.
    Bench {
        image: PathBuf,
        #[arg(long, default_value = "var/models/indra-ocr")]
        models: PathBuf,
        #[arg(long, default_value_t = 3)]
        runs: usize,
        #[arg(long, value_enum, default_value_t = ProfileArg::Indra)]
        profile: ProfileArg,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum ProfileArg {
    Indra,
    Handwriting,
    Sweep,
}

impl From<ProfileArg> for Profile {
    fn from(p: ProfileArg) -> Self {
        match p {
            ProfileArg::Indra => Profile::Indra,
            ProfileArg::Handwriting => Profile::Handwriting,
            ProfileArg::Sweep => Profile::Sweep,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum ModelSet {
    /// det + rec: the minimum for the Latin handwriting path.
    Core,
    /// orient + det + script + rec: the recommended fast path.
    Fast,
    /// Everything present in the model directory.
    All,
}

impl ModelSet {
    fn models(self) -> Vec<ModelKind> {
        match self {
            Self::Core => vec![ModelKind::Det, ModelKind::Rec],
            Self::Fast => vec![ModelKind::Orient, ModelKind::Det, ModelKind::Script, ModelKind::Rec],
            Self::All => ALL_MODELS.to_vec(),
        }
    }
}

/// Drive one page to completion, measuring each inference call.
fn run_page(
    exec: &mut OrtExecutor,
    rgba: &[u8],
    width: usize,
    height: usize,
    cfg: EngineConfig,
    vocabs: BTreeMap<&'static str, Vocab>,
) -> Result<PageResult> {
    let available = exec.available().to_vec();
    if !available.contains(&ModelKind::Det) || !available.contains(&ModelKind::Rec) {
        bail!(
            "det.onnx and rec.onnx are both required; loaded: {:?}",
            available.iter().map(|m| m.filename()).collect::<Vec<_>>()
        );
    }
    let mut session = Session::new(cfg, rgba, width, height, vocabs, available)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    let mut step = session.start();
    // A page has a bounded number of inference calls; this bounds a bug in the
    // state machine rather than trusting it to terminate.
    let mut guard = 0usize;
    loop {
        guard += 1;
        if guard > 4096 {
            bail!("the engine did not terminate after {guard} inference calls");
        }
        step = match step {
            HostStep::Infer(req) => {
                // The engine times this call itself: its mark spans emit-to-feed,
                // so the host's `run` is already inside the stage figure.
                let outputs = exec
                    .run(&req)
                    .map_err(|e| anyhow::anyhow!("inference failed: {e}"))?;
                session.feed(outputs)
            }
            HostStep::Done(result) => return Ok(*result),
            HostStep::Failed(e) => bail!("engine error: {e}"),
        };
    }
}

fn load_meta(dir: &Path) -> Result<Meta> {
    let path = dir.join("meta.json");
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("reading {}: the model directory needs meta.json for the vocabularies", path.display()))?;
    Meta::parse(&text).context("parsing meta.json")
}

fn load_image(path: &Path) -> Result<(Vec<u8>, usize, usize)> {
    let img = image::open(path).with_context(|| format!("decoding {}", path.display()))?;
    let (w, h) = img.dimensions();
    Ok((img.to_rgba8().into_raw(), w as usize, h as usize))
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Inspect { models } => {
            let (mut exec, missing) = OrtExecutor::load(&models, &ALL_MODELS, 1)?;
            println!("model directory: {}", models.display());
            println!("loaded:  {}", exec.loaded().iter().map(|m| m.filename()).collect::<Vec<_>>().join(", "));
            if !missing.is_empty() {
                println!("missing: {}", missing.iter().map(|m| m.filename()).collect::<Vec<_>>().join(", "));
            }
            for kind in exec.loaded().to_vec() {
                let (inputs, outputs) = exec.describe(kind)?;
                println!("\n{}", kind.filename());
                for i in inputs {
                    println!("  in   {i}");
                }
                for o in outputs {
                    println!("  out  {o}");
                }
            }
        }

        Command::Run {
            image,
            profile,
            models,
            set,
            json,
            threads,
            no_deskew,
        } => {
            let (exec, missing) = OrtExecutor::load(&models, &set.models(), threads)?;
            let mut exec = exec;
            let meta = load_meta(&models)?;
            let vocabs = nayen_core::vocabs_from_meta(&meta).map_err(|e| anyhow::anyhow!("{e}"))?;
            let (rgba, w, h) = load_image(&image)?;

            let mut cfg = EngineConfig::from_profile(profile.into());
            cfg.detect.long_side = meta.det_long_side;
            if no_deskew {
                cfg.deskew = false;
            }

            let t0 = Instant::now();
            let result = run_page(&mut exec, &rgba, w, h, cfg, vocabs)?;
            let wall = t0.elapsed().as_secs_f64() * 1000.0;

            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
                return Ok(());
            }

            println!("NAYEN v{}  profile={:?}", nayen_core::VERSION, result.profile);
            println!("page {}x{}   wall {wall:.0} ms", result.width, result.height);
            if result.rotation.abs() > 0.05 {
                println!("deskew {:.1} deg", result.rotation);
            }
            if result.quarter_turns > 0 {
                println!("quarter turns {}", result.quarter_turns);
            }
            println!(
                "models: {}",
                result.models_used.iter().map(|m| m.filename()).collect::<Vec<_>>().join(", ")
            );
            if !missing.is_empty() {
                println!(
                    "not loaded: {}",
                    missing.iter().map(|m| m.filename()).collect::<Vec<_>>().join(", ")
                );
            }
            let t = result.timings;
            println!(
                "stages ms: deskew {:.0}  orient {:.0}  detect {:.0}  crop {:.0}  route {:.0}  recognize {:.0}  laya {:.0}",
                t.deskew, t.orient, t.detect, t.crop, t.route, t.recognize, t.laya
            );
            println!("\n{} lines:", result.lines.len());
            for (i, l) in result.lines.iter().enumerate() {
                let flag = if l.needs_review { "*" } else { " " };
                let text = if l.text.is_empty() { "<no confident reading>" } else { &l.text };
                println!(
                    "{flag}{i:>3}  conf {:.3}  [{:>6.1},{:>6.1},{:>6.1},{:>6.1}]  {}",
                    l.conf, l.box_.x0, l.box_.y0, l.box_.x1, l.box_.y1, text
                );
            }
            if let Some(f) = &result.fields {
                if let Some(dt) = &f.doc_type {
                    println!("\ndoc type (hint only): {dt}");
                }
                for fld in &f.fields {
                    let mark = match fld.valid {
                        Some(true) => "ok",
                        Some(false) => "INVALID",
                        None => "--",
                    };
                    println!("  {:<15} [{mark:>7}] {:?}  conf {:.3}", fld.name, fld.value, fld.conf);
                }
                if let Some(m) = f.car_lar_match {
                    println!("  amount in words vs figures: {}", if m { "AGREE" } else { "MISMATCH" });
                }
                for a in &f.anomalies {
                    println!("  {:?} {}: {}", a.severity, a.code, a.message);
                }
            }
            if !result.notes.is_empty() {
                println!("\nnotes:");
                for n in &result.notes {
                    println!("  - {n}");
                }
            }
            println!("\n--- markdown ---");
            print!("{}", result.markdown);
        }

        Command::Bench {
            image,
            models,
            runs,
            profile,
        } => {
            let (mut exec, _) = OrtExecutor::load(
                &models,
                &[ModelKind::Orient, ModelKind::Det, ModelKind::Script, ModelKind::Rec],
                0,
            )?;
            let meta = load_meta(&models)?;
            let vocabs = nayen_core::vocabs_from_meta(&meta).map_err(|e| anyhow::anyhow!("{e}"))?;
            let (rgba, w, h) = load_image(&image)?;
            let cfg = EngineConfig::from_profile(profile.into());

            let mut walls = Vec::new();
            let mut last: Option<PageResult> = None;
            for _ in 0..runs.max(1) {
                let t0 = Instant::now();
                let r = run_page(&mut exec, &rgba, w, h, cfg.clone(), vocabs.clone())?;
                walls.push(t0.elapsed().as_secs_f64() * 1000.0);
                last = Some(r);
            }
            walls.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let r = last.unwrap();
            println!(
                "{} runs on {}x{}: min {:.0} ms  median {:.0} ms  max {:.0} ms",
                walls.len(),
                w,
                h,
                walls[0],
                walls[walls.len() / 2],
                walls[walls.len() - 1]
            );
            println!("{} lines, {} models", r.lines.len(), r.models_used.len());
            let t = r.timings;
            println!(
                "median-run stages ms: deskew {:.0} orient {:.0} detect {:.0} crop {:.0} route {:.0} recognize {:.0} laya {:.0}",
                t.deskew, t.orient, t.detect, t.crop, t.route, t.recognize, t.laya
            );
            if walls.len() > 1 {
                let pps = 1000.0 / walls[walls.len() / 2];
                println!("throughput {pps:.2} pages/s at the median");
            }
        }
    }
    Ok(())
}
