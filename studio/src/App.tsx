import {useCallback, useEffect, useRef, useState} from 'react'
import type {PointerEvent as ReactPointerEvent} from 'react'
import type {Box, Doc, Evidence, Field, Job, Results, Sample, Template} from './types'
import {history, moveBox, resizeBox, latestResults, importTemplate} from './geometry'
import type {History} from './geometry'
import './App.css'

const PREFIX = '/api/v1'
const authorization = (token: string) => ['Bearer', token].join(' ')
const terminal = new Set(['needs_review','partial','failed','cancelled','completed'])
const fieldTypes = ['text','multiline','enum','decimal','checkbox','date','comb','ifsc','pan','amount_words','amount_figures']
type API = <T>(path: string, init?: RequestInit) => Promise<T>
function download(data: Blob, name: string) {
  const url = URL.createObjectURL(data), a = document.createElement('a')
  a.href = url; a.download = name; a.click(); setTimeout(() => URL.revokeObjectURL(url), 1000)
}
function jsonDownload(value: unknown, name: string) {
  download(new Blob([JSON.stringify(value,null,2)],{type:'application/json'}), name)
}
function ImageAsset({path, token, alt, className}: {path: string; token: string; alt: string; className?: string}) {
  const [asset,setAsset] = useState({path:'',url:''})
  useEffect(() => {
    let object = ''; const controller = new AbortController()
    fetch(PREFIX+path,{headers:{Authorization:authorization(token)},signal:controller.signal})
      .then(r => {if (!r.ok) throw Error('Image unavailable'); return r.blob()})
      .then(blob => {object=URL.createObjectURL(blob);setAsset({path,url:object})}).catch(() => {})
    return () => {controller.abort(); if(object) URL.revokeObjectURL(object)}
  },[path,token])
  return asset.path===path&&asset.url ? <img src={asset.url} alt={alt} className={className}/> : <span className="muted">Image unavailable / loading</span>
}

function Canvas({template, image, selected, select, change, evidence}: {
  template: Template; image: string; selected: string; select: (id: string)=>void;
  change: (value:Template)=>void; evidence: Record<string,Evidence>;
}) {
  const [zoom,setZoom] = useState(1), [rotation,setRotation] = useState(0)
  const [pan,setPan] = useState({x:0,y:0}), [tool,setTool] = useState('edit')
  const [visible,setVisible] = useState(true)
  const svg = useRef<SVGSVGElement>(null)
  const drag = useRef<{x:number;y:number; base:Template; id:string; kind:string; index:number; resize:boolean} | null>(null)
  const [preview,setPreview] = useState<Template|null>(null)
  function point(e: ReactPointerEvent) {
    const matrix=svg.current?.getScreenCTM()
    if (!matrix) return {x:0,y:0}
    return new DOMPoint(e.clientX,e.clientY).matrixTransform(matrix.inverse())
  }
  function start(e: ReactPointerEvent, id: string, kind='roi', index=0, resize=false) {
    if(tool==='pan') return
    e.stopPropagation(); e.preventDefault(); select(id)
    const p=point(e); drag.current={x:p.x,y:p.y,base:template,id,kind,index,resize}
    svg.current?.setPointerCapture(e.pointerId)
  }
  function move(e: ReactPointerEvent) {
    const d=drag.current; if(!d) return
    const p=point(e), dx=p.x-d.x,dy=p.y-d.y
    const next=structuredClone(d.base), f=next.fields.find(f=>f.field_id===d.id)!
    const original=d.kind==='roi' ? f.roi : (d.kind==='cells'?f.cells:f.line_rois)[d.index]
    let box=d.resize ? resizeBox(original,p.x,p.y,next.width_px,next.height_px) :
      moveBox(original,dx,dy,next.width_px,next.height_px)
    if(d.kind==='roi') {
      if(d.resize) {
        box[2]=Math.max(box[2],...f.cells.map(b=>b[2]),...f.line_rois.map(b=>b[2]))
        box[3]=Math.max(box[3],...f.cells.map(b=>b[3]),...f.line_rois.map(b=>b[3]))
      } else {
        const sx=box[0]-original[0],sy=box[1]-original[1]
        f.cells=f.cells.map(b=>moveBox(b,sx,sy,next.width_px,next.height_px))
        f.line_rois=f.line_rois.map(b=>moveBox(b,sx,sy,next.width_px,next.height_px))
      }
      f.roi=box
    } else {
      if(d.resize) box=resizeBox(original,Math.min(p.x,f.roi[2]),Math.min(p.y,f.roi[3]),f.roi[2],f.roi[3])
      else {
        const sx=Math.max(f.roi[0]-original[0],Math.min(Math.round(dx),f.roi[2]-original[2]))
        const sy=Math.max(f.roi[1]-original[1],Math.min(Math.round(dy),f.roi[3]-original[3]))
        box=moveBox(original,sx,sy,next.width_px,next.height_px)
      }
      ;(d.kind==='cells'?f.cells:f.line_rois)[d.index]=box
    }
    setPreview(next)
  }
  function end() {if(preview) change(preview); drag.current=null;setPreview(null)}
  const shown=preview||template
  const panDrag=useRef<{x:number;y:number;px:number;py:number}|null>(null)
  return <section className="viewer" aria-label="Original image and ROI editor">
    <div className="toolbar">
      <button onClick={()=>setZoom(z=>Math.max(.25,z-.25))} aria-label="Zoom out">−</button>
      <output>{Math.round(zoom*100)}%</output>
      <button onClick={()=>setZoom(z=>Math.min(5,z+.25))} aria-label="Zoom in">+</button>
      <button onClick={()=>setRotation(r=>(r+90)%360)}>Rotate {rotation}°</button>
      <button onClick={()=>{setZoom(1);setPan({x:0,y:0});setRotation(0)}}>Fit</button>
      <button aria-pressed={tool==='pan'} onClick={()=>setTool(t=>t==='pan'?'edit':'pan')}>Pan</button>
      <button aria-pressed={tool==='subcrop'} onClick={()=>setTool(t=>t==='subcrop'?'edit':'subcrop')}>Lines / cells</button>
      <label><input type="checkbox" checked={visible} onChange={e=>setVisible(e.target.checked)}/> ROIs</label>
    </div>
    <p className="caption">View rotation only · original upright pixels · half-open [x₀,y₀,x₁,y₁) · drag box / corner to resize</p>
    <div className={'viewport '+(tool==='pan'?'panning':'')} onPointerDown={e=>{
      if(tool==='pan') {e.currentTarget.setPointerCapture(e.pointerId);panDrag.current={x:e.clientX,y:e.clientY,px:pan.x,py:pan.y}}
    }} onPointerMove={e=>{const d=panDrag.current;if(d)setPan({x:d.px+e.clientX-d.x,y:d.py+e.clientY-d.y})}}
      onPointerUp={()=>{panDrag.current=null}} onPointerCancel={()=>{panDrag.current=null}}>
      <svg ref={svg} viewBox={`0 0 ${shown.width_px} ${shown.height_px}`}
        style={{transform:`translate(${pan.x}px,${pan.y}px) rotate(${rotation}deg) scale(${zoom})`}}
        onPointerMove={move} onPointerUp={end} onPointerCancel={()=>{drag.current=null;setPreview(null)}}
        role="img" aria-label="Full resolution document, editable field rectangles">
        <image href={image} width={shown.width_px} height={shown.height_px}/>
        {visible&&shown.fields.map(f=>{
          const active=f.field_id===selected, result=evidence[f.field_id]
          const status=result?.validation_issues?.length?'bad':result?.status==='reviewed'&&!result.review?.stale?'good':'pending'
          const rect=(b:Box,kind:string,index:number)=> <g key={kind+index}>
            <rect x={b[0]} y={b[1]} width={b[2]-b[0]} height={b[3]-b[1]}
              className={`roi ${status} ${active?'selected':''} ${kind!=='roi'?'subroi':''}`}
              style={{pointerEvents:kind==='roi'?(tool==='subcrop'?'none':'auto'):(tool==='subcrop'?'auto':'none')}}
              tabIndex={0} role="button" aria-label={`${f.label} ${kind} ${index+1}`}
              onFocus={()=>select(f.field_id)} onPointerDown={e=>start(e,f.field_id,kind,index)}
              onKeyDown={e=>{
                const shifts:Record<string,[number,number]>={ArrowLeft:[-1,0],ArrowRight:[1,0],ArrowUp:[0,-1],ArrowDown:[0,1]}
                if(!shifts[e.key])return;e.preventDefault()
                const n=structuredClone(template),field=n.fields.find(x=>x.field_id===f.field_id)!
                const [dx,dy]=shifts[e.key], step=e.shiftKey?10:1
                if(kind==='roi') {
                  const b=moveBox(field.roi,dx*step,dy*step,n.width_px,n.height_px)
                  const sx=b[0]-field.roi[0],sy=b[1]-field.roi[1]
                  field.roi=b;field.cells=field.cells.map(v=>moveBox(v,sx,sy,n.width_px,n.height_px))
                  field.line_rois=field.line_rois.map(v=>moveBox(v,sx,sy,n.width_px,n.height_px));change(n)
                }
              }}/>
            {active&&<rect className="handle" x={b[2]-9} y={b[3]-9} width="18" height="18"
              style={{pointerEvents:kind==='roi'?(tool==='subcrop'?'none':'auto'):(tool==='subcrop'?'auto':'none')}}
              aria-label={`Resize ${f.label} ${kind}`} onPointerDown={e=>start(e,f.field_id,kind,index,true)}/>}
            {kind!=='roi'&&active&&<text x={b[0]+5} y={b[1]+20} className="box-label">{index+1}</text>}
          </g>
          return <g key={f.field_id}>{rect(f.roi,'roi',0)}
            {active&&f.line_rois.map((b,i)=>rect(b,'line_rois',i))}
            {active&&f.cells.map((b,i)=>rect(b,'cells',i))}
            <text x={f.roi[0]} y={f.roi[1]-9} className="box-label">{f.label}</text>
          </g>
        })}
      </svg>
    </div>
    <div className="legend"><span className="good-dot">━ Human-reviewed</span><span className="pending-dot">┄ Needs review / unavailable</span><span className="bad-dot">╍ Validation issue</span></div>
  </section>
}

function FieldEditor({field, update, remove}: {field:Field;update:(f:Field)=>void;remove:()=>void}) {
  const [boxText,setBoxText]=useState(''),[boxError,setBoxError]=useState('')
  // External canvas drags change the selected field's ordered crop coordinates.
  // oxlint-disable-next-line react/set-state-in-effect
  useEffect(()=>{setBoxText(JSON.stringify(field.line_rois.length?field.line_rois:field.cells));setBoxError('')},[field])
  return <div className="field-editor">
    <label>Label<input value={field.label} onChange={e=>update({...field,label:e.target.value})}/></label>
    <label>Type<select aria-label="Type" value={field.type} onChange={e=>update({...field,type:e.target.value})}>{fieldTypes.map(t=><option key={t}>{t}</option>)}</select></label>
    <div className="inline"><label><input type="checkbox" checked={field.numeric} onChange={e=>update({...field,numeric:e.target.checked})}/> Numeric normalization only</label>
      <label><input type="checkbox" checked={field.required} onChange={e=>update({...field,required:e.target.checked})}/> Required</label></div>
    <label>Original pixel rectangle [x₀,y₀,x₁,y₁)
      <div className="box-inputs">{field.roi.map((v,i)=><input key={i} aria-label={['Left x','Top y','Right x','Bottom y'][i]} type="number" value={v}
        onChange={e=>{const roi=[...field.roi] as Box;roi[i]=Number(e.target.value);update({...field,roi})}}/>)}</div>
    </label>
    <label>Enum choices (comma separated)<input value={field.choices.join(',')} onChange={e=>update({...field,choices:e.target.value.split(',').map(s=>s.trim()).filter(Boolean)})}/></label>
    <label>Ordered line / comb-cell rectangles (JSON)
      <textarea aria-label="Ordered line / comb-cell rectangles (JSON)" rows={3} value={boxText} onChange={e=>setBoxText(e.target.value)}/>
    </label>
    <div className="inline">{(['line_rois','cells'] as const).map(kind=><button key={kind} onClick={()=>{
      try {const boxes=JSON.parse(boxText)
        if(!Array.isArray(boxes)||!boxes.every(b=>Array.isArray(b)&&b.length===4&&b.every(Number.isInteger)))throw Error('Use arrays of four integers')
        update({...field,line_rois:kind==='line_rois'?boxes:[],cells:kind==='cells'?boxes:[]});setBoxError('')
      }catch(e){setBoxError(String(e))}
    }}>Apply {kind==='cells'?'comb cells':'lines'}</button>)}</div>
    <p className="caption">Read top-to-bottom in this array order. Lines and cells are exclusive. Backend validates containment; no automatic alignment.</p>
    {boxError&&<p role="alert">{boxError}</p>}
    <button className="danger" onClick={remove}>Delete ROI</button>
  </div>
}

type UploadItem={id:string;file:File;state:string;progress:number;template:Template}
function UploadQueue({token,template,added,report}: {token:string;template:Template|null;added:(d:Doc)=>void;report:(s:string)=>void}) {
  const [items,setItems]=useState<UploadItem[]>([])
  const requests=useRef(new Map<string,XMLHttpRequest>())
  const queue=useRef<UploadItem[]>([]), busy=useRef(false), alive=useRef(true)
  useEffect(()=>{alive.current=true;const active=requests.current;return()=>{alive.current=false;queue.current=[];active.forEach(x=>x.abort())}},[])
  function patch(id:string,values:Partial<UploadItem>) {if(alive.current)setItems(rows=>rows.map(r=>r.id===id?{...r,...values}:r))}
  function pump() {
    if(busy.current||!queue.current.length||!alive.current)return
    busy.current=true;const item=queue.current.shift()!, xhr=new XMLHttpRequest()
    requests.current.set(item.id,xhr);patch(item.id,{state:'uploading',progress:0})
    xhr.open('POST',`${PREFIX}/documents?template_id=${encodeURIComponent(item.template.template_id)}&template_version=${encodeURIComponent(item.template.template_version)}`)
    xhr.setRequestHeader('Authorization',authorization(token))
    xhr.setRequestHeader('Content-Type',item.file.type)
    xhr.timeout=35000
    xhr.upload.onprogress=e=>{if(e.lengthComputable)patch(item.id,{progress:Math.round(e.loaded/e.total*100)})}
    xhr.upload.onload=()=>patch(item.id,{state:'server decoding'})
    xhr.onload=()=>{
      if(xhr.status===201) {patch(item.id,{state:'stored',progress:100});added(JSON.parse(xhr.responseText))}
      else {let message=`HTTP ${xhr.status}`;try{message=JSON.parse(xhr.responseText).message}catch{/* no response body */}
        patch(item.id,{state:'failed'});report(`${item.file.name}: ${message}`)}
    }
    xhr.onerror=()=>patch(item.id,{state:'failed'})
    xhr.ontimeout=()=>patch(item.id,{state:'failed'})
    xhr.onabort=()=>{patch(item.id,{state:'cancelled'});report('Upload cancelled locally. If decoding had started, refresh documents to check whether the server already stored it.')}
    xhr.onloadend=()=>{requests.current.delete(item.id);busy.current=false;pump()}
    xhr.send(item.file)
  }
  function files(list:FileList|null) {
    if(!list||!template)return
    const rows=Array.from(list).filter(file=>{
      if(!['image/png','image/jpeg','application/pdf'].includes(file.type)||file.size>10485760) {
        report(`${file.name}: only PNG/JPEG/single-page PDF up to 10 MiB`);return false
      }return true
    }).map(file=>({id:crypto.randomUUID(),file,state:'queued',progress:0,template:structuredClone(template)}))
    setItems(r=>[...r,...rows]);queue.current.push(...rows);pump()
  }
  return <section className="upload" onDragOver={e=>e.preventDefault()} onDrop={e=>{e.preventDefault();files(e.dataTransfer.files)}}>
    <label className="upload-label">＋ Drop scans or choose files
      <input aria-label="Upload scans" type="file" multiple accept="image/png,image/jpeg,application/pdf" disabled={!template}
        onChange={e=>{files(e.target.files);e.target.value=''}}/>
    </label><p className="caption">Raw bytes · max 10 MiB · exact template dimensions · no guessed scaling</p>
    {items.map(item=><div className="queue-item" key={item.id}><span>{item.file.name}</span><small>{item.state}</small>
      <progress max={100} value={item.progress} aria-label={`${item.file.name} bytes sent`}/>
      <small>{item.progress}% bytes sent (not OCR progress)</small>
      {['queued','uploading','server decoding'].includes(item.state)&&<button onClick={()=>{
        queue.current=queue.current.filter(r=>r.id!==item.id);requests.current.get(item.id)?.abort();patch(item.id,{state:'cancelled'})
      }}>Cancel upload</button>}
      {['failed','cancelled'].includes(item.state)&&<button onClick={()=>{const retry={...item,state:'queued',progress:0};patch(item.id,retry);queue.current.push(retry);pump()}}>Retry upload</button>}
    </div>)}
  </section>
}

type SettingsData={revision:number;values:Record<string,unknown>;sources:Record<string,string>;runtime_keys:string[]}
type SchemaData={schema:{properties:Record<string,{type?:string;enum?:unknown[];description?:string;minimum?:number;maximum?:number}>};runtime_keys:string[];help:unknown}
function ModelCatalog() {
  return <section className="model-catalog" aria-labelledby="catalog-title">
    <div className="section-heading"><div><span className="eyebrow">LOCAL MODEL HARNESS / CATALOG</span><h2 id="catalog-title">Adapters, not promises.</h2></div><span className="badge">OFFLINE ONLY</span></div>
    <p>Implementation status is separate from artifact availability. This catalog does not install, download, run or benchmark models.</p>
    <div className="model-grid">
      <article className="model-card implemented"><span className="model-index">01 / IMPLEMENTED ADAPTER</span><h3>TrOCR</h3>
        <code>microsoft/trocr-base-handwritten</code><p>The only implemented inference adapter. Approved local weights are required; missing weights fail explicitly.</p>
        <span className="model-status">Readiness: see live API report below</span></article>
      <article className="model-card"><span className="model-index">02 / REQUESTED CANDIDATE</span><h3>LightOnOCR 3</h3>
        <code>lightonai/LightOnOCR-3-0.8B<br/>lightonai/LightOnOCR-3-1B<br/>lightonai/LightOnOCR-3-4B</code>
        <p>Official upstream identities confirmed. Upstream documents local vLLM OpenAI-compatible calls and grounding coordinates normalized to 0–1000.</p>
        <span className="badge">NOT INTEGRATED</span><p>No adapter or tested weights here. This Studio blocks vLLM; no HF Space inference endpoint has been verified.</p>
        <span className="model-status">Identity known / adapter pending</span></article>
      <article className="model-card"><span className="model-index">03 / REQUESTED CANDIDATE</span><h3>Xiaomi OCR 0</h3>
        <code>SeerRay-Lab/Xiaomi-OCR-0</code><p>Upstream identity confirmed: 0.8B, derived from Qwen3.5. No local adapter is implemented. Runtime compatibility remains unverified.</p>
        <span className="badge">NOT INTEGRATED</span><p>The upstream Space is a project page and assets, not a verified inference endpoint. No inference URL is configured or guessed.</p>
        <span className="model-status">Identity known / inference unavailable here</span></article>
    </div>
    <p className="caption">No measured comparison or accuracy claim is available. Candidate entries do not change job routing. No cloud requests or remote fallback.</p>
  </section>
}
function SettingsPanel({api,report}: {api:API;report:(s:string)=>void}) {
  const [settings,setSettings]=useState<SettingsData|null>(null),[schema,setSchema]=useState<SchemaData|null>(null)
  const [query,setQuery]=useState(''),[level,setLevel]=useState('Basic'),[draft,setDraft]=useState<Record<string,unknown>>({})
  const [models,setModels]=useState<unknown>(null)
  useEffect(()=>{api('/settings').then(v=>setSettings(v as SettingsData)).catch(e=>report(e.message))
    api('/settings/schema').then(v=>setSchema(v as SchemaData)).catch(e=>report(e.message))
    api('/models').then(setModels).catch(e=>report(e.message))
  },[api,report])
  async function save(values:Record<string,unknown>) {
    if(!settings)return
    await api('/settings',{method:'PATCH',body:JSON.stringify({revision:settings.revision,values})})
    setSettings(await api('/settings'));setDraft({});report('Settings saved atomically; apply to new jobs only.')
  }
  return <section className="settings"><span className="eyebrow">SYSTEM / OPERATOR CONTROL</span><h1>Configuration</h1>
    <p>No provider key fields. OpenRouter and vLLM are blocked, not connected. No off-host fallback.</p>
    <ModelCatalog/>
    <div className="inline">{['Basic','Advanced','Expert'].map(v=><button key={v} aria-pressed={level===v} onClick={()=>setLevel(v)}>{v}</button>)}
      <input aria-label="Search settings" placeholder="Search settings…" value={query} onChange={e=>setQuery(e.target.value)}/></div>
    {settings&&schema&&<>
      <h3>Runtime · admin only · new jobs</h3>
      {Object.entries(schema.schema.properties).filter(([key])=>schema.runtime_keys.includes(key)&&key.includes(query)).map(([key,s])=>
        <label key={key}>{key}<input aria-label={key} type="number" min={s.minimum} max={s.maximum}
          value={String(draft[key]??settings.values[key]??'')}
          onChange={e=>setDraft(v=>({...v,[key]:Number(e.target.value)}))}/>
          <small>{s.description||'Bounded runtime setting'} · {s.minimum}–{s.maximum} · source: {settings.sources[key]}</small>
        </label>)}
      <button onClick={()=>save(draft).catch(e=>report(e.message))} disabled={!Object.keys(draft).length}>Save runtime settings</button>
      <button onClick={()=>api('/settings/export').then(v=>jsonDownload(v,'icr-redacted-config.json')).catch(e=>report(e.message))}>Export redacted configuration</button>
      <label>Import runtime configuration<input type="file" accept=".json" onChange={async e=>{
        try {const file=e.target.files?.[0];if(!file)return;const imported=JSON.parse(await file.text())
          const values=Object.fromEntries(Object.entries(imported.values||{}).filter(([key])=>schema.runtime_keys.includes(key)))
          await save(values)
        }catch(e){report(String(e))}
      }}/></label>
      {level!=='Basic'&&<><h3>Restart-only · offline operator configuration</h3>
        <p>Read-only here. Change reviewed YAML/environment and restart. Secrets are environment-only and absent.</p>
        {Object.entries(schema.schema.properties).filter(([key])=>!schema.runtime_keys.includes(key)&&key.includes(query)).map(([key,s])=>
          <div className="setting-readonly" key={key}><strong>{key}</strong><code>{JSON.stringify(settings.values[key])}</code>
            <small>{s.description} · source: {settings.sources[key]}</small></div>)}</>}
      {level==='Expert'&&<><h3>Actual schema help</h3><pre>{JSON.stringify(schema.help,null,2)}</pre>
        <h3>Administration</h3><button onClick={()=>api('/admin/ledger/reset',{method:'POST'}).then(()=>report('Failure quarantine reset and audited.')).catch(e=>report(e.message))}>Reset failure quarantine</button>
        <button onClick={()=>api('/admin/retention',{method:'POST'}).then(()=>report('Retention applied.')).catch(e=>report(e.message))}>Apply retention now</button></>}
    </>}
    <h3>Real model availability</h3>{models===null?<p className="muted">Awaiting local API model report. Readiness not yet established.</p>:<pre>{JSON.stringify(models,null,2)}</pre>}
    <p className="caption">Artifact presence is not inference-tested readiness. No downloads at startup. No confidence calibration.</p>
  </section>
}

export default function App() {
  const [token,setToken]=useState(''),[entry,setEntry]=useState(''),[connected,setConnected]=useState(false)
  const [authEpoch,setAuthEpoch]=useState(0)
  const [error,setError]=useState(''),[notice,setNotice]=useState(''),[tab,setTab]=useState('studio')
  const [templates,setTemplates]=useState<Template[]>([]),[samples,setSamples]=useState<Sample[]>([]),[docs,setDocs]=useState<Doc[]>([])
  const [hist,setHist]=useState<History|null>(null),[doc,setDoc]=useState<Doc|null>(null),[results,setResultsState]=useState<Results|null>(null)
  const activeDocument=useRef('')
  const setResults=useCallback((next:Results|null)=>{
    if(next&&next.document_id!==activeDocument.current)return
    setResultsState(current=>latestResults(current,next))
  },[])
  const [selected,setSelected]=useState(''),[image,setImage]=useState(''),[job,setJob]=useState<Job|null>(null)
  const [aligned,setAligned]=useState(false),[evidenceOpen,setEvidenceOpen]=useState(false),[value,setValue]=useState<string|boolean|null>('')
  const [newVersion,setNewVersion]=useState(''),[health,setHealth]=useState<Record<string,unknown>|null>(null)
  const [events,setEvents]=useState(0),[busy,setBusy]=useState(false)
  const editBuffer=useRef({key:'',dirty:false})
  const requestControllers=useRef(new Set<AbortController>()),selectionSeq=useRef(0)
  const report=useCallback((message:string)=>setNotice(message),[])
  const api=useCallback<API>(async(path,init={})=>{
    const controller=new AbortController();requestControllers.current.add(controller)
    try {
      const r=await fetch(PREFIX+path,{...init,signal:controller.signal,headers:{
        Authorization:authorization(token),...(init.body?{'Content-Type':'application/json'}:{}),...init.headers,
      }})
      if(!r.ok){const data=await r.json().catch(()=>({message:`HTTP ${r.status}`}));throw Error(`${data.message} (${r.status})`)}
      return r.status===204?null:r.json()
    } finally {requestControllers.current.delete(controller)}
  },[token])
  const run=useCallback(async(fn:()=>Promise<void>)=>{
    setError('');setBusy(true)
    try{await fn()}catch(e){if((e as Error).name!=='AbortError')setError((e as Error).message)}
    finally{setBusy(false)}
  },[])
  async function refresh() {setDocs((await api<{items:Doc[]}>('/documents')).items)}
  const loadDoc=useCallback(async(id:string)=>{
    const seq=++selectionSeq.current
    const d=await api<Doc>(`/documents/${id}`), r=await api<Results>(`/documents/${id}/results`)
    const response=await fetch(`${PREFIX}/documents/${id}/image`,{headers:{Authorization:authorization(token)}})
    if(!response.ok)throw Error('Document image unavailable')
    const url=URL.createObjectURL(await response.blob())
    if(seq!==selectionSeq.current){URL.revokeObjectURL(url);return}
    activeDocument.current=d.id
    setImage(url);setDoc(d);setResults(r);setHist({past:[],present:d.template!,future:[]})
    setSelected(d.template!.fields[0]?.field_id||'');setAligned(false);setJob(null);setEvents(0)
    setNewVersion('');setTab('studio')
    if(r.job_id){const current=await api<Job>(`/jobs/${r.job_id}`);if(seq===selectionSeq.current)setJob(current)}
  },[api,token,setResults])
  useEffect(()=>()=>{if(image)URL.revokeObjectURL(image)},[image])
  useEffect(()=>{
    if(!token)return
    // Authentication starts an external API synchronization, not a derived render.
    // oxlint-disable-next-line react/set-state-in-effect
    run(async()=>{
      const [t,s,d]=await Promise.all([api<{items:Template[]}>('/templates'),api<{items:Sample[]}>('/samples'),api<{items:Doc[]}>('/documents')])
      setTemplates(t.items);setSamples(s.items);setDocs(d.items);setConnected(true)
      if(t.items[0]){setHist({past:[],present:t.items[0],future:[]});setSelected(t.items[0].fields[0]?.field_id||'')}
    })
  },[token,api,run,authEpoch])
  useEffect(()=>{
    fetch('/readyz').then(r=>r.json()).then(setHealth).catch(()=>setHealth({api:'unreachable'}))
  },[])
  const jobId=job?.id, documentId=doc?.id
  useEffect(()=>{
    if(!jobId||!documentId)return
    const controller=new AbortController();let cursor=0
    const id=documentId
    async function follow() {
      try {
        while(!controller.signal.aborted) {
          const response=await fetch(`${PREFIX}/jobs/${jobId}/events?after=${cursor}`,{
            headers:{Authorization:authorization(token),Accept:'text/event-stream'},signal:controller.signal})
          if(!response.ok)throw Error(`Event stream unavailable (${response.status})`)
          const reader=response.body!.getReader(), decoder=new TextDecoder()
          let buffer=''
          while(true) {
            const {value,done}=await reader.read();if(done)break
            buffer+=decoder.decode(value,{stream:true});let boundary
            while((boundary=buffer.indexOf('\n\n'))>=0) {
              const event=buffer.slice(0,boundary);buffer=buffer.slice(boundary+2)
              const seq=event.match(/^id:\s*(\d+)/m)
              if(seq) {
                cursor=Number(seq[1]);setEvents(n=>n+1)
                const current=await api<Job>(`/jobs/${jobId}`)
                if(controller.signal.aborted)return
                setJob(current)
                const result=await api<Results>(`/documents/${id}/results`)
                if(controller.signal.aborted)return
                setResults(result)
              }
            }
          }
          const current=await api<Job>(`/jobs/${jobId}`)
          if(controller.signal.aborted)return
          setJob(current)
          if(terminal.has(current.state)) {
            const result=await api<Results>(`/documents/${id}/results`)
            if(!controller.signal.aborted)setResults(result)
            break
          }
        }
      }catch(e){if(!controller.signal.aborted)setError((e as Error).message+' — use Refresh evidence to reconnect.')}
    }
    follow();return()=>controller.abort()
  },[jobId,documentId,api,token,setResults])
  const template=hist?.present, field=template?.fields.find(f=>f.field_id===selected), evidence=results?.fields[selected]
  const uploadTemplate=template&&hist?.past.length===0&&templates.some(t=>
    t.template_id===template.template_id&&t.template_version===template.template_version)?template:null
  // Stream refreshes must never erase an operator's unsaved correction.
  // oxlint-disable-next-line react/set-state-in-effect
  useEffect(()=>{
    const key=`${documentId}/${selected}`
    if(editBuffer.current.key!==key)editBuffer.current={key,dirty:false}
    if(!editBuffer.current.dirty)setValue(evidence?.value??'')
  },[evidence,selected,documentId])
  function change(t:Template) {setHist(h=>h?history(h,{type:'set',value:t}):{past:[],present:t,future:[]})}
  function undo() {setHist(h=>h?history(h,{type:'undo'}):h)}
  function redo() {setHist(h=>h?history(h,{type:'redo'}):h)}
  const saveTemplate=useCallback(async()=>{
    if(!hist)return
    if(!newVersion.trim())throw Error('Enter a new immutable version before saving.')
    const next={...hist.present,template_version:newVersion.trim()}
    const saved=await api<Template>('/templates',{method:'POST',body:JSON.stringify(next)})
    setTemplates(t=>[...t,saved]);setHist({past:[],present:saved,future:[]})
    report('Immutable template saved. Existing document unchanged: re-upload a scan with this version to apply and rerun.')
  },[hist,newVersion,api,report])
  useEffect(()=>{
    function keys(e:KeyboardEvent) {
      const target=e.target as HTMLElement
      if(target.closest('input,textarea,select,[contenteditable="true"]'))return
      if((e.ctrlKey||e.metaKey)&&e.key.toLowerCase()==='z'){e.preventDefault();if(e.shiftKey)redo();else undo()}
      if((e.ctrlKey||e.metaKey)&&e.key.toLowerCase()==='s'){e.preventDefault();run(saveTemplate)}
    }window.addEventListener('keydown',keys);return()=>window.removeEventListener('keydown',keys)
  },[saveTemplate,run])
  function logout() {
    selectionSeq.current++;activeDocument.current='';requestControllers.current.forEach(c=>c.abort());setToken('');setEntry('');setConnected(false)
    setDocs([]);setSamples([]);setTemplates([]);setDoc(null);setResults(null);setJob(null);setHist(null);setImage('');setError('');setNotice('')
  }
  async function exportResult(format:string,mode:string) {
    if(!doc)return
    const r=await fetch(`${PREFIX}/documents/${doc.id}/export?format=${format}&mode=${mode}`,{headers:{Authorization:authorization(token)}})
    if(!r.ok){const body=await r.json();throw Error(body.message)}
    download(await r.blob(),`icr-${mode}.${format}`)
  }
  return <div className="app">
    <a className="skip" href="#main">Skip to workspace</a>
    <header><div className="brand"><span className="brand-icon">▧</span><div><strong>ICR <span>STUDIO</span></strong><small>LOCAL DOCUMENT WORKSPACE</small></div></div>
      <div className="header-meta"><span className="privacy">● Local only</span><span>Human review required</span>
        <span className="readiness">Inference: {String(health?.inference??'checking')}</span></div>
      {connected&&<button onClick={logout}>Lock workspace</button>}
    </header>
    {!connected?<main className="login" id="main">
      <section className="login-intro"><span className="eyebrow"><span className="signal"/> PRIVATE BY DESIGN / LOCAL BY DEFAULT</span>
        <h1>Your documents.<br/>Your <em>review.</em></h1>
        <p className="login-description">A controlled workspace for document recognition.<br/>Original pixels. Inspectable evidence. Human decisions.</p>
        <div className="login-blueprint" aria-hidden="true"><div className="blueprint-top"><span>DOCUMENT / INSPECTION LAYER</span><span>ROI</span></div>
          <div className="blueprint-page"><div className="blueprint-lines"/><div className="blueprint-roi"><span>FIELD REGION</span><i/><i/><i/></div><div className="blueprint-lines short"/><div className="blueprint-cells">{Array.from({length:8},(_,i)=><i key={i}/>)}</div></div>
          <div className="blueprint-bottom"><span>ORIGINAL PIXELS</span><span>HUMAN IN THE LOOP ↗</span></div></div>
        <div className="login-principles"><span><b>01</b> Inspect the source</span><span><b>02</b> Review every field</span><span><b>03</b> Export deliberately</span></div>
      </section>
      <section className="unlock-panel" aria-labelledby="unlock-title"><div className="unlock-heading"><span className="eyebrow">OPERATOR ACCESS</span><span className="access-mark" aria-hidden="true">↗</span></div>
      <h2 id="unlock-title">Unlock your workspace.</h2><p>Encrypted local storage. Explicit human decisions.<br/>No automatic approval. No cloud fallback.</p>
      <form onSubmit={e=>{e.preventDefault();setToken(entry);setEntry('');setAuthEpoch(n=>n+1)}}>
        <label>Operator bearer token<input type="password" autoComplete="off" value={entry} onChange={e=>setEntry(e.target.value)} required minLength={32} aria-describedby="token-help"/></label>
        <small id="token-help">Paste from your private server environment. Kept only in memory; reload locks the workspace.</small>
        <button className="primary" type="submit" disabled={busy}>Unlock local workspace →</button>
      </form>{error&&<p role="alert" className="error">{error}</p>}
      <div className="access-status"><span className="signal"/><span>LOCAL API</span><strong>{String(health?.api??'checking')}</strong></div>
      <p className="caption">Synthetic demo fixtures are not real handwriting. Missing weights produce real errors, never sample-derived predictions.</p>
      <div className="session-note"><span aria-hidden="true">↳</span> Credentials stay in memory. Your review stays yours.</div></section>
    </main>:<>
      <nav aria-label="Workspace"><button aria-current={tab==='studio'?'page':undefined} onClick={()=>setTab('studio')}>01 / Review studio</button>
        <button aria-current={tab==='gallery'?'page':undefined} onClick={()=>setTab('gallery')}>02 / Synthetic gallery</button>
        <button aria-current={tab==='settings'?'page':undefined} onClick={()=>setTab('settings')}>03 / Settings & models</button>
        <span>{docs.length} stored documents · token in memory</span></nav>
      <div className="messages" aria-live="polite">{notice&&<p>{notice}<button onClick={()=>setNotice('')} aria-label="Dismiss notice">×</button></p>}
        {error&&<p role="alert" className="error">{error}<button onClick={()=>setError('')} aria-label="Dismiss error">×</button></p>}</div>
      <main id="main">
        {tab==='gallery'&&<section className="gallery-section"><span className="eyebrow">SYNTHETIC / NOT REAL HANDWRITING</span><h1>A safe place to inspect the workflow.</h1>
          <p>Five importable upright fixtures. The sixth requires manual alignment and is deliberately blocked. Labels are never used as OCR.</p>
          <div className="gallery">{samples.map((s,i)=><article key={s.sample_id}>
            <ImageAsset path={`/samples/${s.sample_id}/image`} token={token} alt={`Synthetic ${s.form_kind} fixture ${i+1}`}/>
            <div><span className="eyebrow">SYNTHETIC {String(i+1).padStart(2,'0')}</span><h2>{s.form_kind}</h2><p>{s.width_px} × {s.height_px} px</p>
              {s.alignment_required?<><strong className="bad-dot">Alignment required — import blocked</strong><p>Correct the scan externally, review alignment, then upload. No guessed transformation.</p></>:
                <button className="primary" disabled={busy} onClick={()=>run(async()=>{const d=await api<Doc>(`/samples/${s.sample_id}/import`,{method:'POST'});await refresh();await loadDoc(d.id)})}>Import fixture →</button>}
            </div>
          </article>)}</div></section>}
        {tab==='settings'&&<SettingsPanel api={api} report={report}/>}
        {tab==='studio'&&<><section className="session-overview" aria-label="Workspace overview">
          <div className="overview-title"><span className="eyebrow">OPERATIONS / REVIEW DESK</span><h2>Document control.</h2><span className="caption">Live local records. No inferred accuracy.</span></div>
          <dl className="overview-metrics"><div><dt>Stored documents</dt><dd>{docs.length.toString().padStart(2,'0')}</dd></div>
            <div><dt>Human reviewed</dt><dd>{docs.filter(d=>d.review_state==='reviewed').length.toString().padStart(2,'0')}</dd></div>
            <div><dt>Saved templates</dt><dd>{templates.length.toString().padStart(2,'0')}</dd></div>
            <div><dt>Selected document fields</dt><dd>{results?`${Object.values(results.fields).filter(f=>f.review&&!f.review.stale).length} / ${Object.keys(results.fields).length}`:'—'}<small>explicitly reviewed</small></dd></div></dl>
        </section><div className="workspace">
          <aside className="sidebar"><span className="eyebrow">DOCUMENTS & TEMPLATES</span>
            <label>Upload / editor template<select aria-label="Upload / editor template" value={template?`${template.template_id}@${template.template_version}`:''} onChange={e=>{
              const t=templates.find(t=>`${t.template_id}@${t.template_version}`===e.target.value)
              if(t){setHist({past:[],present:structuredClone(t),future:[]});setSelected(t.fields[0]?.field_id||'')}
            }}>{templates.map(t=><option key={`${t.template_id}@${t.template_version}`} value={`${t.template_id}@${t.template_version}`}>{t.form_kind} · {t.template_version} · {t.template_id}</option>)}</select></label>
            <UploadQueue key={token} token={token} template={uploadTemplate} report={report} added={d=>{refresh();run(()=>loadDoc(d.id))}}/>
            {!uploadTemplate&&<p className="caption">Upload paused: save a new template version or reset the editor draft first.</p>}
            <div className="section-heading"><h2>Document queue</h2><button onClick={()=>run(refresh)}>Refresh</button></div>
            {!docs.length&&<p className="muted">No scans yet. Upload or import a synthetic fixture.</p>}
            {docs.map(d=><button className={'doc-item '+(doc?.id===d.id?'active':'')} key={d.id} onClick={()=>run(()=>loadDoc(d.id))}>
              <strong>{d.template_id.replace('synthetic-','').toUpperCase()}</strong><span>{d.id.slice(0,10)}…</span>
              <small>{d.review_state} {d.synthetic?'· synthetic':''}</small>
            </button>)}
            <button onClick={()=>setTab('gallery')}>Browse synthetic gallery →</button>
            <hr/><div className="section-heading"><h2>Template editor</h2><small>{template?.fields.length??0} fields</small></div>
            <div className="inline"><button disabled={!hist?.past.length} onClick={undo}>Undo</button><button disabled={!hist?.future.length} onClick={redo}>Redo</button>
              <button onClick={()=>{if(doc?.template){setHist({past:[],present:structuredClone(doc.template),future:[]})}else if(template){const t=templates.find(t=>t.template_id===template.template_id&&t.template_version===template.template_version);if(t)change(structuredClone(t))}}}>Reset</button></div>
            <small>Ctrl/⌘ Z · Shift Z · Ctrl/⌘ S saves new version</small>
            <label>New immutable version<input placeholder="e.g. 1.0.1" value={newVersion} maxLength={32} onChange={e=>setNewVersion(e.target.value)}/></label>
            <button onClick={()=>run(saveTemplate)} disabled={busy}>Save new template version</button>
            <p className="caption">Editing never changes this document's snapshot. Save a new version, select it, and re-upload to apply.</p>
            <div className="inline"><button onClick={()=>template&&jsonDownload(template,'icr-template.json')}>Export template</button>
              <label className="file-button">Import JSON<input type="file" accept=".json" onChange={async e=>{
                try{const file=e.target.files?.[0];if(!file)return;if(file.size>1048576)throw Error('Template JSON limit: 1 MiB')
                  const value=importTemplate(JSON.parse(await file.text()))
                  change(value);setSelected(value.fields[0]?.field_id||'');report('Imported editor draft. Server performs strict validation when saving a new version.')
                }catch(e){setError(String(e))}
              }}/></label></div>
          </aside>
          <section className="center">
            <div className="workspace-heading"><div><span className="eyebrow">ORIGINAL PIXELS / HUMAN DECISIONS</span><h1>{doc?`${doc.template?.form_kind} document`:'Review workspace'}</h1>
              <p>{doc?`${doc.width_px} × ${doc.height_px} px · ${doc.synthetic?'Synthetic fixture':'Uploaded scan'} · ${results?.review_state}`:'Upload a scan or open the synthetic gallery to begin.'}</p></div>
              {doc&&<span className={'badge '+(results?.review_state==='reviewed'?'reviewed':'')}>{results?.review_state==='reviewed'?'HUMAN REVIEWED':'DRAFT UNTIL REVIEWED'}</span>}
            </div>
            {doc&&template&&image&&template.width_px===doc.width_px&&template.height_px===doc.height_px?<Canvas key={doc.id} template={template} image={image} selected={selected} select={setSelected} change={change} evidence={results?.fields||{}}/>:
              <div className="empty"><div className="empty-coordinate" aria-hidden="true">[ x₀, y₀, x₁, y₁ )</div><span aria-hidden="true">▧</span><span className="eyebrow">AWAITING DOCUMENT</span><h2>A clear view of every field.</h2><p>Full-resolution evidence, editable ROIs,<br/>and an honest trail from model to review.</p><button className="primary" onClick={()=>setTab('gallery')}>Open synthetic gallery →</button><small>Or upload a scan from the document panel.</small></div>}
            {doc&&template&&(template.template_id!==doc.template_id||template.template_version!==doc.template_version||hist!.past.length>0)&&<p className="caption">Editor preview only. OCR and evidence still use document snapshot {doc.template_id} / {doc.template_version}. Re-upload with a saved version to apply.</p>}
            {doc&&<section className="job-panel"><div className="section-heading"><h2>Recognition queue</h2><span>{job?.state||'Not submitted'}</span></div>
              <label><input type="checkbox" checked={aligned} onChange={e=>setAligned(e.target.checked)}/> I checked original upright alignment against this document's immutable template.</label>
              <div className="inline"><button className="primary" disabled={!aligned||busy||!!job&&!terminal.has(job.state)} onClick={()=>run(async()=>{
                const j=await api<{id:string}>(`/documents/${doc.id}/jobs`,{method:'POST',body:JSON.stringify({idempotency_key:crypto.randomUUID(),alignment_confirmed:true,remote_consent:false})})
                setEvents(0);setJob(await api(`/jobs/${j.id}`))
              })}>Queue local OCR</button>
                {job&&!terminal.has(job.state)&&<button onClick={()=>run(async()=>{await api(`/jobs/${job.id}/cancel`,{method:'POST'});setJob(await api(`/jobs/${job.id}`))})}>Cancel OCR</button>}
                <button onClick={()=>run(async()=>{setResults(await api(`/documents/${doc.id}/results`));if(job)setJob(await api(`/jobs/${job.id}`))})}>Refresh evidence</button>
              </div>
              {job&&<><p>{job.progress.done} / {job.progress.total} fields processed · {events} actual SSE events received</p><progress max={job.progress.total||1} value={job.progress.done} aria-label="Actual OCR field progress"/>
                {job.error&&<pre className="error">{JSON.stringify(job.error,null,2)}</pre>}</>}
              <p className="caption">Only local TrOCR adapter. Missing weights fail honestly. No automated tiling, deskew, external fallback or model-derived confidence.</p>
            </section>}
          </section>
          <aside className="review-panel"><span className="eyebrow">FIELD INSPECTOR</span><h2>Review, don't assume.</h2>
            {template&&<>
              <label>Selected field<select aria-label="Selected field" value={selected} onChange={e=>setSelected(e.target.value)}>{template.fields.map(f=><option key={f.field_id} value={f.field_id}>{f.label}</option>)}</select></label>
              <button onClick={()=>{const id=`field_${Date.now()}`;const next=structuredClone(template);next.fields.push({
                field_id:id,label:'New field',type:'text',required:false,page:1,roi:[50,50,Math.min(450,next.width_px),Math.min(150,next.height_px)],
                line_rois:[],cells:[],numeric:false,choices:[],
              });change(next);setSelected(id)}} disabled={template.fields.length>=100}>＋ Create ROI</button>
              {field&&<><details><summary>Edit ROI & recognition type</summary><FieldEditor field={field} update={f=>{
                const next=structuredClone(template);next.fields=next.fields.map(v=>v.field_id===f.field_id?f:v);change(next)
              }} remove={()=>{const next={...template,fields:template.fields.filter(f=>f.field_id!==selected)};change(next);setSelected(next.fields[0]?.field_id||'')}}/></details>
                {doc&&evidence?<><div className="crop"><ImageAsset token={token} path={`/documents/${doc.id}/crops/${selected}`} alt={`Original immutable crop for ${field.label}`}/></div>
                  <div className="evidence-summary"><span className={'badge '+(evidence.status==='reviewed'?'reviewed':'')}>{evidence.status}</span><small>Confidence: unavailable</small></div>
                  <label>Immutable model raw text<textarea aria-label="Immutable model raw text" rows={2} readOnly value={evidence.raw_text??''} placeholder="No model text available"/></label>
                  <label>Human-reviewed value{field.type==='checkbox'?<select aria-label="Human-reviewed value" value={value===true?'true':value===false?'false':''} onChange={e=>{editBuffer.current.dirty=true;setValue(e.target.value===''?null:e.target.value==='true')}}>
                    <option value="">Unknown</option><option value="true">Checked</option><option value="false">Unchecked</option></select>:
                    <textarea aria-label="Human-reviewed value" rows={3} maxLength={4096} value={typeof value==='string'?value:''} onChange={e=>{editBuffer.current.dirty=true;setValue(e.target.value)}}/>}</label>
                  <button className="primary" disabled={busy} onClick={()=>run(async()=>{
                    const updated=await api<Results>(`/documents/${doc.id}/fields/${selected}`,{
                      method:'PATCH',body:JSON.stringify({revision:results!.revision,value}),
                    })
                    editBuffer.current.dirty=false;setResults(updated)
                  })}>Save human correction</button>
                  {evidence.review&&<p className="caption">Reviewed by {evidence.review.reviewer} · {new Date(evidence.review.at*1000).toLocaleString()} {evidence.review.stale?'· STALE — reconfirm after rerun':''}</p>}
                  {!!evidence.validation_issues.length&&<p className="error">Validation: {evidence.validation_issues.join(', ')}</p>}
                  <button onClick={()=>setEvidenceOpen(true)}>Compare raw / reviewed / provenance →</button>
                </>:<p className="muted">No evidence for this editor field in the immutable document. Save a new template and re-upload.</p>}
              </>}
            </>}
            {doc&&results&&<><hr/><h3>Document review</h3><p>Amount reconciliation: <strong>{results.amount_reconciliation.status}</strong></p>
              <p>{Object.values(results.fields).filter(f=>f.review&&!f.review.stale).length} / {Object.keys(results.fields).length} fields explicitly reviewed</p>
              <button disabled={busy} onClick={()=>run(async()=>{setResults(await api(`/documents/${doc.id}/review`,{method:'POST',body:JSON.stringify({revision:results.revision})}));await refresh()})}>Finalize human review</button>
              <p className="caption">Blocked until every field is reviewed, validation passes, and no job is active.</p>
              <h3>Export real results</h3>{['draft','reviewed'].map(mode=><div className="inline" key={mode}><span>{mode.toUpperCase()}</span>{['json','csv'].map(format=><button key={format} onClick={()=>run(()=>exportResult(format,mode))}>{format.toUpperCase()}</button>)}</div>)}
              <p className="caption">Drafts are unverified. CSV formula prefixes are neutralized server-side. JSON preserves raw evidence and corrections.</p>
              <button className="danger" onClick={()=>{if(window.confirm('Permanently remove this document and its jobs/reviews from the application? Backups may remain.'))run(async()=>{
                await api(`/documents/${doc.id}`,{method:'DELETE'});activeDocument.current='';selectionSeq.current++;setDoc(null);setResults(null);setJob(null);setImage('');await refresh()
              })}}>Delete document</button>
            </>}
          </aside>
        </div></>}
      </main>
      {evidenceOpen&&<div className="drawer-backdrop" onClick={()=>setEvidenceOpen(false)}><section className="drawer" role="dialog" aria-modal="true" aria-label="Evidence comparison" onClick={e=>e.stopPropagation()} onKeyDown={e=>{
        if(e.key==='Escape')setEvidenceOpen(false)
        if(e.key==='Tab'){e.preventDefault();e.currentTarget.querySelector('button')?.focus()}
      }}>
        <button autoFocus onClick={()=>setEvidenceOpen(false)}>Close comparison ×</button><h2>{field?.label} / evidence</h2>
        <p>Only actual stored evidence. No alternate-model candidates exist unless recorded. OpenRouter / vLLM unavailable.</p>
        <h3>Raw → normalized → reviewed</h3><pre>{JSON.stringify({raw:evidence?.raw_text,normalized:evidence?.normalized_value,review:evidence?.review},null,2)}</pre>
        <h3>Model, attempts, timing & provenance</h3><pre>{JSON.stringify(evidence,null,2)}</pre>
      </section></div>}
    </>}
    <footer><span>ICR BANK / STUDIO</span><span>No accuracy claim · no automatic approval · no regulatory certification</span><span>API {String(health?.api??'checking')}</span></footer>
  </div>
}
