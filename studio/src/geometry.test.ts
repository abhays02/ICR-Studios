import test from 'node:test'
import assert from 'node:assert/strict'
import {moveBox, resizeBox, history, latestResults, importTemplate} from './geometry.ts'
import type {Template, Results} from './types.ts'

test('drag uses original half-open pixels and clamps without changing dimensions', () => {
  assert.deepEqual(moveBox([10,20,30,40], 200,-30,100,100), [80,0,100,20])
  assert.deepEqual(moveBox([10,20,30,40], 1.6,2.2,100,100), [12,22,32,42])
})
test('resize is integer, positive, and image bounded', () => {
  assert.deepEqual(resizeBox([10,20,30,40], 500,-2,100,100), [10,20,100,21])
})
test('undo and redo preserve immutable snapshots; new edit clears redo', () => {
  const a = {template_version:'a'} as Template, b = {...a, template_version:'b'}
  const initial = {past:[], present:a, future:[]}
  const next = history(initial, {type:'set',value:b})
  assert.equal(history(next,{type:'undo'}).present, a)
  assert.equal(history(history(next,{type:'undo'}),{type:'redo'}).present,b)
  assert.deepEqual(history(history(next,{type:'undo'}),{type:'set',value:b}).future,[])
})
test('late stream snapshots cannot replace newer human corrections', () => {
  const corrected = {document_id:'one',revision:4} as Results
  assert.equal(latestResults(corrected,{document_id:'one',revision:3} as Results),corrected)
  assert.equal(latestResults(corrected,null),null)
  const other={document_id:'two',revision:1} as Results
  assert.equal(latestResults(corrected,other),other)
})
const templateDraft = {
  template_id:'test',template_version:'1',form_kind:'CAR',width_px:100,height_px:200,
  fields:[{field_id:'a',label:'A',type:'text',roi:[0,0,100,100]}],
}
test('import fills optional crop collections without mutating the input', () => {
  const actual=importTemplate(templateDraft)
  assert.deepEqual(actual.fields[0].line_rois,[])
  assert.equal('line_rois' in templateDraft.fields[0],false)
})
test('import rejects malformed/duplicate fields and out-of-bounds half-open crops', () => {
  assert.throws(()=>importTemplate({...templateDraft,fields:[null]}))
  assert.throws(()=>importTemplate({...templateDraft,fields:[...templateDraft.fields,...templateDraft.fields]}))
  assert.throws(()=>importTemplate({...templateDraft,fields:[{...templateDraft.fields[0],roi:[0,0,101,100]}]}))
  assert.throws(()=>importTemplate({...templateDraft,fields:[{...templateDraft.fields[0],cells:[[0,0,10,101]]}]}))
})
