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
