export type Box = [number, number, number, number]
export type Field = {
  field_id: string; label: string; type: string; required: boolean; page: number;
  roi: Box; line_rois: Box[]; cells: Box[]; numeric: boolean; choices: string[];
}
export type Template = {
  template_id: string; template_version: string; schema_version: string; form_kind: string;
  width_px: number; height_px: number; page: number; coordinate_space: string;
  bbox_convention: string; synthetic: boolean; fields: Field[];
}
export type Doc = {
  id: string; revision: number; width_px: number; height_px: number; template_id: string;
  template_version: string; review_state: string; synthetic: boolean; template?: Template;
}
export type Evidence = {
  field_id: string; raw_text: string | null; normalized_value: string | boolean | null;
  value: string | boolean | null; status: string; confidence: null; confidence_kind: string;
  issues: string[]; validation_issues: string[]; model_used: string | null;
  review: { value: string | boolean | null; stale?: boolean; reviewer: string; at: number } | null;
  provenance?: Record<string, unknown>;
}
export type Results = {
  document_id: string;
  revision: number; review_state: string; fields: Record<string, Evidence>;
  job_id: string | null; job_state: string | null; warnings: string[];
  amount_reconciliation: {status: string; match: boolean | null};
}
export type Job = { id: string; state: string; error: unknown; progress: {done: number; total: number} }
export type Sample = {sample_id: string; form_kind: string; width_px: number; height_px: number; alignment_required: boolean}

/* ---------------------------------------------------------------------------
   NAYEN v1 engine host contract mirrors.
   Source of truth: engine/src/types.ts (kept in sync with crates/nayen-wasm
   via the engine's check:contract script). Mirrored here so the offline
   Studio bundle stays hermetic; do not drift these shapes locally.
   ------------------------------------------------------------------------ */
export type Profile = 'indra' | 'handwriting' | 'sweep'
export type ModelKind = 'orient' | 'det' | 'script' | 'rec' | 'rec_world' | 'rec_cjk' | 'rec_rtl' | 'order'
export type ScriptGroup = 'base' | 'cjk' | 'rtl' | 'other'
export type EngineConfig = {
  profile: Profile; minConf: number; orient: boolean;
  routeScripts: boolean; laya: boolean; maxPageSide: number;
}
export type LineBox = { x0: number; y0: number; x1: number; y1: number; score: number }
export type LineResult = {
  box: LineBox; text: string; conf: number; script: ScriptGroup;
  needsReview: boolean; charX: number[];
}
export type ExtractedField = { name: string; value: string; conf: number; valid: boolean | null; rule: string }
export type Anomaly = { code: string; message: string; severity: 'info' | 'warn' | 'fault'; lineIndex: number | null }
export type FieldExtraction = {
  docType: string | null; fields: ExtractedField[];
  anomalies: Anomaly[]; carLarMatch: boolean | null;
}
export type StageTimings = {
  orient: number; detect: number; crop: number;
  recognize: number; laya: number; total: number;
}
export type PageResult = {
  width: number; height: number; lines: LineResult[]; markdown: string;
  fields: FieldExtraction | null; timings: StageTimings; profile: Profile;
  modelsUsed: ModelKind[];
}
/** Mirrors DEFAULT_CONFIG in engine/src/types.ts. */
export const DEFAULT_ENGINE_CONFIG: EngineConfig = {
  profile: 'indra', minConf: 0.5, orient: true,
  routeScripts: true, laya: true, maxPageSide: 1800,
}
export const ENGINE_PROFILES: Profile[] = ['indra', 'handwriting', 'sweep']
export const STAGE_ORDER: Array<keyof StageTimings> = ['orient', 'detect', 'crop', 'recognize', 'laya', 'total']
