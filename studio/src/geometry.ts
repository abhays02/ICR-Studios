import type { Box, Template, Results } from './types.ts'

export function latestResults(current: Results | null, next: Results | null): Results | null {
  if (!next || !current || current.document_id !== next.document_id) return next
  return next.revision >= current.revision ? next : current
}
export function importTemplate(value: unknown): Template {
  const t = value as Template
  if (!t || typeof t !== 'object' || typeof t.template_id !== 'string' ||
      typeof t.template_version !== 'string' || typeof t.form_kind !== 'string' ||
      !Number.isInteger(t.width_px) || !Number.isInteger(t.height_px) ||
      t.width_px < 1 || t.height_px < 1 || t.width_px * t.height_px > 25000000 ||
      !Array.isArray(t.fields) || t.fields.length > 100) throw Error('Invalid template structure or image dimensions')
  const ids = new Set<string>()
  const within = (box: Box, outer: Box) => Array.isArray(box) && box.length === 4 &&
    box.every(Number.isInteger) && box[0] >= outer[0] && box[1] >= outer[1] &&
    box[2] <= outer[2] && box[3] <= outer[3] && box[0] < box[2] && box[1] < box[3]
  const next = structuredClone(t)
  for (const f of next.fields) {
    if (!f || typeof f.field_id !== 'string' || !/^[A-Za-z0-9_]{1,64}$/.test(f.field_id) ||
        ids.has(f.field_id) || typeof f.label !== 'string' || typeof f.type !== 'string' ||
        !within(f.roi,[0,0,t.width_px,t.height_px])) throw Error('Invalid or duplicate field / original-pixel ROI')
    ids.add(f.field_id)
    f.line_rois ??= []; f.cells ??= []; f.choices ??= []; f.numeric ??= false; f.required ??= false; f.page ??= 1
    if (!Array.isArray(f.line_rois) || !Array.isArray(f.cells) || !Array.isArray(f.choices) ||
        !f.choices.every(v=>typeof v==='string') || f.line_rois.length && f.cells.length ||
        ![...f.line_rois,...f.cells].every(b=>within(b,f.roi))) throw Error('Invalid ordered line/cell crops')
  }
  return next
}

export function moveBox(box: Box, dx: number, dy: number, width: number, height: number): Box {
  dx = Math.max(-box[0], Math.min(Math.round(dx), width - box[2]))
  dy = Math.max(-box[1], Math.min(Math.round(dy), height - box[3]))
  return [box[0] + dx, box[1] + dy, box[2] + dx, box[3] + dy]
}
export function resizeBox(box: Box, x: number, y: number, width: number, height: number): Box {
  return [box[0], box[1], Math.max(box[0] + 1, Math.min(width, Math.round(x))),
    Math.max(box[1] + 1, Math.min(height, Math.round(y)))]
}
export type History = {past: Template[]; present: Template; future: Template[]}
export function history(state: History, action: {type: 'set'; value: Template} | {type: 'undo' | 'redo'}): History {
  if (action.type === 'set') return {past: [...state.past.slice(-49), state.present], present: action.value, future: []}
  if (action.type === 'undo' && state.past.length) return {
    past: state.past.slice(0, -1), present: state.past.at(-1)!, future: [state.present, ...state.future],
  }
  if (action.type === 'redo' && state.future.length) return {
    past: [...state.past, state.present], present: state.future[0], future: state.future.slice(1),
  }
  return state
}
