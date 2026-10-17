import { useEffect, useRef, useState } from 'react'
import type { ChangeEvent, DragEvent } from 'react'

type Model = { id: string; label: string; available: boolean; reason?: string }
type Region = { label: string; box: [number, number, number, number] }
type Extraction = { model: string; text: string; data: unknown; regions?: Region[]; elapsed_ms: number }
type Phase = 'idle' | 'loading' | 'running' | 'done' | 'error'
const MAX_BYTES = 10 * 1024 * 1024
const OFFLINE_STUDIO = 'http://127.0.0.1:8000'
const LOCAL_STUDIO_NOTE = 'Requires the offline Studio server running locally on port 8000'
const SAMPLE = {
  notice: 'Illustrative sample — not a model result',
  document_type: 'invoice',
  supplier: 'NORTHLINE SYSTEMS',
  invoice_number: 'INV-2026-0042',
  issued: '2026-10-01',
  currency: 'USD',
  items: [
    { description: 'Document workflow design', amount: 1200 },
    { description: 'Integration workshop', amount: 450 },
  ],
  total: 1650,
}

function Icon({ name = 'arrow', size = 18 }: { name?: string; size?: number }) {
  const paths: Record<string, string> = {
    arrow: 'M4 12h16m-6-6 6 6-6 6',
    upload: 'M12 16V3m-5 5 5-5 5 5M4 16v5h16v-5',
    copy: 'M8 8h12v13H8zM16 8V3H3v13h5',
    check: 'm5 12 4 4L19 6',
    bolt: 'm13 2-9 12h7l-1 8L21 9h-8z',
    layers: 'm12 3 10 5-10 5L2 8zm-10 9 10 5 10-5M2 16l10 5 10-5',
    shield: 'm12 2 8 3v6c0 5-8 11-8 11S4 16 4 11V5zm-4 9 3 3 5-6',
    terminal: 'm4 5 6 7-6 7m9 0h7',
    file: 'M14 2H5v20h14V7zm0 0v5h5M8 12h8M8 16h6',
    link: 'm10 14 4-4m-6 5-1 1a4 4 0 0 1-6-6l4-4a4 4 0 0 1 6 0m2 3 1-1a4 4 0 0 1 6 6l-4 4a4 4 0 0 1-6 0',
  }
  return <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d={paths[name] || paths.arrow} /></svg>
}

function Invoice() {
  return <svg className="invoice-svg" viewBox="0 0 420 530" role="img" aria-label="Synthetic Northline Systems invoice, INV-2026-0042, total 1,650 USD. Authored illustration, not a real document.">
    <rect x="1" y="1" width="418" height="528" rx="3" fill="#F1F3F5" />
    <path d="M30 55V30h25v25H30zm6-6h13V36H36z" fill="#0B0C0E" />
    <g fill="#0B0C0E" fontFamily="monospace">
      <text x="66" y="41" fontSize="12" fontWeight="bold">NORTHLINE</text><text x="66" y="55" fontSize="9" letterSpacing="2">SYSTEMS</text>
      <text x="30" y="115" fontSize="32" fontWeight="bold" letterSpacing="-1">INVOICE</text>
      <text x="30" y="139" fontSize="10">INV-2026-0042</text>
      <text x="280" y="110" fontSize="9">ISSUED</text><text x="280" y="129" fontSize="11">01 OCT 2026</text>
      <text x="30" y="190" fontSize="9" fill="#64748B">BILLED TO</text>
      <text x="30" y="211" fontSize="12">Example Company</text>
      <text x="30" y="229" fontSize="10" fill="#64748B">100 Sample Street</text>
      <text x="30" y="245" fontSize="10" fill="#64748B">Illustration City, 00000</text>
      <text x="30" y="291" fontSize="9">DESCRIPTION</text><text x="335" y="291" fontSize="9">AMOUNT</text>
      <text x="30" y="327" fontSize="10">Document workflow design</text><text x="329" y="327" fontSize="11">1,200.00</text>
      <text x="30" y="359" fontSize="10">Integration workshop</text><text x="341" y="359" fontSize="11">450.00</text>
      <text x="30" y="420" fontSize="10">TOTAL DUE / USD</text><text x="266" y="423" fontSize="23" fontWeight="bold">1,650.00</text>
      <text x="30" y="487" fontSize="9" fill="#64748B">SYNTHETIC DOCUMENT · FOR DEMONSTRATION ONLY</text>
    </g>
    <g stroke="#CBD5E1"><path d="M30 268h360M30 302h360M30 341h360M30 378h360M30 448h360" /></g>
    <g stroke="#9EFF00" fill="none" strokeWidth="1.5"><rect x="23" y="119" width="132" height="30" /><rect x="258" y="396" width="139" height="37" /></g>
    <g fill="#0B0C0E" fontFamily="monospace" fontSize="8"><rect x="23" y="107" width="74" height="13" fill="#9EFF00" /><text x="27" y="116">SAMPLE FIELD</text><rect x="258" y="384" width="74" height="13" fill="#9EFF00" /><text x="262" y="393">SAMPLE FIELD</text></g>
  </svg>
}

function Connection({ active }: { active: boolean }) {
  return <div className={`connection ${active ? 'is-active' : ''}`} aria-hidden="true">
    <svg viewBox="0 0 170 400" preserveAspectRatio="none">
      <defs><linearGradient id="flow"><stop stopColor="#9EFF00" stopOpacity="0.1" /><stop offset=".5" stopColor="#9EFF00" /><stop offset="1" stopColor="#9EFF00" stopOpacity="0.1" /></linearGradient></defs>
      {[80, 140, 200, 260, 320].map((y, i) => <path key={y} className={`strand strand-${i}`} d={`M0 ${y} C75 ${y},95 ${400 - y},170 ${400 - y}`} />)}
    </svg>
    <span className="flow-node"><Icon name="bolt" size={22} /></span>
    <span className="flow-caption">{active ? 'PROCESSING' : 'PIPELINE'}</span>
  </div>
}

function xmlEscape(value: string) {
  return value.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;').replace(/'/g, '&apos;')
}
function toXml(value: unknown, depth = 1): string {
  const pad = '  '.repeat(depth)
  if (value === null) return `${pad}<null />`
  if (Array.isArray(value)) return value.map(item => `${pad}<item>\n${toXml(item, depth + 1)}\n${pad}</item>`).join('\n')
  if (typeof value === 'object') return Object.entries(value as Record<string, unknown>).map(([key, item]) =>
    `${pad}<field name="${xmlEscape(key)}">\n${toXml(item, depth + 1)}\n${pad}</field>`).join('\n')
  return `${pad}${xmlEscape(String(value))}`
}
function Code({ text, format }: { text: string; format: 'JSON' | 'XML' }) {
  return <pre className="code-view" tabIndex={0} aria-label={`${format} output`}><code>{text.split('\n').map((line, index) =>
    <span className="code-line" key={index}><span className="line-number" aria-hidden="true">{index + 1}</span><span>{line.split(/("(?:\\.|[^"\\])*"|\b(?:true|false|null|-?\d+(?:\.\d+)?)\b|<\/?[\w]+|&[a-z]+;)/g).map((part, i) =>
      <span key={i} className={part.startsWith('"') ? (line.slice(line.indexOf(part) + part.length).trimStart().startsWith(':') ? 'token-key' : 'token-string') : /^(?:true|false|null|-?\d|<|&)/.test(part) ? 'token-value' : undefined}>{part}</span>)}</span></span>,
  )}</code></pre>
}
function parseExtraction(value: unknown): Extraction {
  if (!value || typeof value !== 'object') throw new Error('The gateway returned an invalid response.')
  const item = value as Record<string, unknown>
  if (typeof item.model !== 'string' || typeof item.text !== 'string' || !('data' in item) || typeof item.elapsed_ms !== 'number' || !Number.isFinite(item.elapsed_ms) || item.elapsed_ms < 0) throw new Error('The gateway response is missing required extraction fields.')
  let regions: Region[] | undefined
  if (Array.isArray(item.regions)) regions = item.regions.filter((region): region is Region => {
    if (!region || typeof region !== 'object') return false
    const candidate = region as Record<string, unknown>
    return typeof candidate.label === 'string' && Array.isArray(candidate.box) && candidate.box.length === 4
      && candidate.box.every(n => typeof n === 'number' && Number.isFinite(n) && n >= 0 && n <= 1)
      && candidate.box[2] > candidate.box[0] && candidate.box[3] > candidate.box[1]
  })
  return { model: item.model, text: item.text, data: item.data, regions, elapsed_ms: item.elapsed_ms }
}
async function responseError(response: Response) {
  try {
    const body = await response.json()
    return typeof body.detail === 'string' ? body.detail : `Gateway request failed (${response.status}).`
  } catch { return `Gateway request failed (${response.status}).` }
}

export default function Demo() {
  const [file, setFile] = useState<File | null>(null)
  const [preview, setPreview] = useState<string | null>(null)
  const [consent, setConsent] = useState(false)
  const [models, setModels] = useState<Model[]>([])
  const [model, setModel] = useState('')
  const [phase, setPhase] = useState<Phase>('idle')
  const [error, setError] = useState('')
  const [modelError, setModelError] = useState('')
  const [result, setResult] = useState<Extraction | null>(null)
  const [format, setFormat] = useState<'JSON' | 'XML'>('JSON')
  const [notice, setNotice] = useState('')
  const [dragging, setDragging] = useState(false)
  const [modelAttempt, setModelAttempt] = useState(0)
  const inputRef = useRef<HTMLInputElement>(null)
  const controller = useRef<AbortController | null>(null)
  const currentRequest = useRef(0)

  useEffect(() => {
    if (!preview) return
    return () => { URL.revokeObjectURL(preview) }
  }, [preview])
  useEffect(() => () => { controller.current?.abort() }, [])
  useEffect(() => {
    if (!consent) return
    const abort = new AbortController()
    let live = true
    fetch('/demo-api/models', { signal: abort.signal, credentials: 'omit', cache: 'no-store', redirect: 'error' })
      .then(async response => {
        if (!response.ok) throw new Error(await responseError(response))
        const body = await response.json()
        if (!Array.isArray(body.models) || !body.models.every((item: Model) => item && typeof item.id === 'string' && typeof item.label === 'string' && typeof item.available === 'boolean' && (item.reason === undefined || typeof item.reason === 'string'))) throw new Error('The gateway returned an invalid model list.')
        if (live) {
          setModels(body.models)
          setModel(body.models.find((item: Model) => item.available)?.id || '')
          if (!body.models.some((item: Model) => item.available)) setModelError('No models are available on this gateway. The illustrative sample remains available.')
          setPhase('idle')
        }
      }).catch((cause: unknown) => {
        if (live && !abort.signal.aborted) { setModelError(cause instanceof Error ? cause.message : 'Model gateway unavailable.'); setPhase('idle') }
      })
    return () => { live = false; abort.abort() }
  }, [consent, modelAttempt])

  function cancel(message = 'Request canceled. No extraction result was generated.') {
    currentRequest.current += 1
    controller.current?.abort()
    controller.current = null
    setPhase('idle')
    setNotice(message)
  }
  function acceptFile(next: File | undefined) {
    if (!next) return
    if (phase === 'running') cancel('Previous request canceled because the document changed.')
    setError('')
    setNotice('')
    if (!['image/png', 'image/jpeg', 'application/pdf'].includes(next.type)) { setError('Choose a PNG, JPEG, or PDF file. Other formats are not supported.'); return }
    if (next.size > MAX_BYTES || next.size === 0) { setError('Choose a non-empty file no larger than 10 MiB.'); return }
    setPreview(next.type.startsWith('image/') ? URL.createObjectURL(next) : null)
    setFile(next)
    setResult(null)
    if (phase !== 'loading') setPhase('idle')
  }
  function onFile(event: ChangeEvent<HTMLInputElement>) {
    acceptFile(event.target.files?.[0])
    event.target.value = ''
  }
  function onDrop(event: DragEvent<HTMLElement>) {
    event.preventDefault()
    setDragging(false)
    if (event.dataTransfer.files.length !== 1) { setError('Please select one document at a time.'); return }
    acceptFile(event.dataTransfer.files[0])
  }
  async function run() {
    if (!file || !consent || !models.some(item => item.id === model && item.available) || phase === 'running') return
    const abort = new AbortController()
    controller.current = abort
    const request = ++currentRequest.current
    setPhase('running'); setError(''); setNotice(''); setResult(null)
    try {
      const response = await fetch('/demo-api/extract', {
        method: 'POST', body: file, signal: abort.signal, credentials: 'omit', redirect: 'error',
        headers: { 'Content-Type': file.type, 'X-Model-Id': model, 'X-Cloud-Consent': 'true' },
      })
      if (!response.ok) throw new Error(await responseError(response))
      const extraction = parseExtraction(await response.json())
      if (request !== currentRequest.current) return
      setResult(extraction); setPhase('done')
    } catch (cause) {
      if (abort.signal.aborted || request !== currentRequest.current) return
      setError(cause instanceof Error ? cause.message : 'Extraction failed. Please try again.'); setPhase('error')
    } finally {
      if (request === currentRequest.current) controller.current = null
    }
  }
  const output = result || (!file ? SAMPLE : { notice: phase === 'running' ? 'Request in progress. No result yet.' : 'No extraction result. Run with explicit cloud consent to process this document.' })
  const outputText = format === 'JSON' ? JSON.stringify(output, null, 2) : `<extraction>\n${toXml(output)}\n</extraction>`
  async function copy(text: string, message: string) {
    try { await navigator.clipboard.writeText(text); setNotice(message) }
    catch { setNotice('Clipboard unavailable. You can select and copy the output manually.') }
  }
  function tryOwn() {
    document.getElementById('sandbox')?.scrollIntoView({ behavior: window.matchMedia('(prefers-reduced-motion: reduce)').matches ? 'instant' : 'smooth' })
    inputRef.current?.focus({ preventScroll: true })
  }
  const running = phase === 'running'

  return <div className="demo-shell">
    <header className="site-header">
      <a className="brand" href="/demo/index.html" aria-label="ICR Studios home"><span className="brand-mark">i<span>c</span>r</span><span className="brand-name">ICR<span>STUDIOS</span></span></a>
      <nav aria-label="Main navigation"><a href="#sandbox">Playground</a><a href="#architecture">Architecture</a><a href="#workbench">The studio</a></nav>
      <a className="header-cta" href={OFFLINE_STUDIO} title={LOCAL_STUDIO_NOTE}>Local studio <Icon size={15} /></a>
    </header>
    <main>
      <section className="hero max-w-7xl mx-auto" aria-labelledby="hero-title">
        <div className="hero-coordinate" aria-hidden="true">SYS.01 / DOCUMENT INTELLIGENCE</div>
        <div className="eyebrow"><span className="status-dot" /> LOCAL-FIRST. STRUCTURE-OBSESSED.</div>
        <h1 id="hero-title">Documents in.<br /><span>Intelligence out.</span></h1>
        <p className="hero-description">Turn the unstructured into the actionable.<br />A modular OCR workbench for the documents that don’t fit the template.</p>
        <div className="hero-actions"><button className="button primary" onClick={tryOwn}>Try your own document <Icon /></button><a className="button ghost" href={OFFLINE_STUDIO} title={LOCAL_STUDIO_NOTE}>Open local studio <Icon name="terminal" /></a></div>
        <div className="hero-notes"><span><Icon name="shield" size={13} /> Local studio requires a running server.</span><span><Icon name="layers" size={13} /> Model-agnostic by design.</span></div>
        <div className="hero-cross" aria-hidden="true">+</div>
      </section>

      <section className="sandbox-section" id="sandbox" aria-labelledby="sandbox-title">
        <div className="section-heading"><div><span className="section-index">01 / LIVE SANDBOX</span><h2 id="sandbox-title">See the structure beneath.</h2></div><span className="technical-label"><span className="status-dot" /> {running ? 'GATEWAY REQUEST ACTIVE' : 'ILLUSTRATIVE UNTIL YOU RUN'}</span></div>
        <div className="sandbox-grid">
          <div className={`document-panel panel ${dragging ? 'dragging' : ''}`} onDragOver={event => { event.preventDefault(); setDragging(true) }} onDragLeave={() => setDragging(false)} onDrop={onDrop}>
            <div className="panel-heading"><span><Icon name="file" size={14} /> INPUT DOCUMENT</span><span className="panel-tag">{file ? 'YOUR FILE · LOCAL PREVIEW' : 'SYNTHETIC SAMPLE'}</span></div>
            <div className={`document-stage ${file ? 'has-file' : ''}`}>
              {!file ? <div className="invoice-paper"><Invoice /></div> : preview ? <div className="uploaded-image">
                <img src={preview} alt={`Local preview of ${file.name}`} />
                {result?.regions?.map((region, i) => <div key={i} className="actual-region" style={{ left: `${region.box[0] * 100}%`, top: `${region.box[1] * 100}%`, width: `${(region.box[2] - region.box[0]) * 100}%`, height: `${(region.box[3] - region.box[1]) * 100}%` }} title={region.label}><span>{region.label}</span></div>)}
              </div> : <div className="pdf-preview"><Icon name="file" size={46} /><strong>PDF ready to process</strong><span>Inline PDF preview is not enabled.</span><span>No document content has been uploaded yet.</span></div>}
              {!file && <span className="paper-caption">Authored illustration / not a model detection</span>}
            </div>
            <div className="file-strip"><span className="file-name">{file ? file.name : 'northline_invoice.sample.svg'}</span><span>{file ? `${(file.size / 1024).toFixed(0)} KB` : 'ILLUSTRATION'}</span>{file && <button className="text-button" onClick={() => { cancel('Illustrative sample restored.'); setFile(null); setPreview(null); setResult(null); setError('') }}>Reset</button>}</div>
            {file && <div className="region-note">{result?.regions?.length ? `${result.regions.length} gateway regions (normalized x1, y1, x2, y2).${file.type === 'application/pdf' ? ' PDF overlay unavailable; coordinates are in the output.' : ''}` : 'Detection overlays unavailable: no region boxes have been returned.'}</div>}
          </div>
          <Connection active={running} />
          <div className="output-panel panel">
            <div className="panel-heading"><span><span className="braces">{'{ }'}</span> STRUCTURED OUTPUT</span><div className="format-switch" role="group" aria-label="Output format">{(['JSON', 'XML'] as const).map(item => <button key={item} aria-pressed={format === item} className={format === item ? 'selected' : ''} onClick={() => setFormat(item)}>{item}</button>)}</div></div>
            <div className="output-status"><span className={result ? 'status-dot' : 'neutral-dot'} />{result ? `GATEWAY RESULT · ${result.model}` : file ? 'AWAITING REAL EXTRACTION' : 'ILLUSTRATIVE SAMPLE · NOT A MODEL RESULT'}<button className="icon-button" aria-label={`Copy ${format} output`} onClick={() => copy(outputText, `${format} output copied.`)}><Icon name="copy" size={15} /></button></div>
            <Code text={outputText} format={format} />
            <div className="output-bottom"><span>{result ? `${result.elapsed_ms.toLocaleString()} ms · gateway-reported time` : 'No measured performance or accuracy claims.'}</span><span className="technical-label">UTF-8</span></div>
          </div>
        </div>
        <div className="run-console">
          <div className="upload-control"><label htmlFor="demo-upload"><Icon name="upload" size={19} /><span><strong>Drop a document. Find its structure.</strong><small>PNG, JPEG, PDF · 10 MiB maximum · one file</small></span></label><input ref={inputRef} id="demo-upload" type="file" accept="image/png,image/jpeg,application/pdf" onChange={onFile} /></div>
          <div className="model-control"><label htmlFor="model-select">EXTRACTION MODEL</label><select id="model-select" value={model} disabled={!consent || phase === 'loading' || running || !models.length} onChange={event => setModel(event.target.value)}>
            {!model && <option value="">{!consent ? 'Enable cloud consent to discover models' : phase === 'loading' ? 'Checking gateway…' : 'No available model'}</option>}
            {models.map(item => <option key={item.id} value={item.id} disabled={!item.available}>{item.label}{!item.available ? ` — unavailable${item.reason ? `: ${item.reason}` : ''}` : ''}</option>)}
          </select></div>
          {running ? <button className="button cancel-button" onClick={() => cancel()}>Cancel request <span aria-hidden="true">×</span></button> : <button className="button primary run-button" disabled={!file || !consent || !model || phase === 'loading' || !models.some(item => item.id === model && item.available)} onClick={run}>Run extraction <Icon name="bolt" size={17} /></button>}
          <label className="consent-control"><input type="checkbox" checked={consent} onChange={event => {
            setConsent(event.target.checked)
            if (event.target.checked) { setPhase('loading'); setModelError('') }
            if (!event.target.checked) { cancel('Cloud consent withdrawn. An in-flight upload may already have reached the gateway.'); setModels([]); setModel(''); setModelError('') }
          }} /><span>I consent to cloud processing when I click <strong>Run extraction</strong>. Enabling this contacts the model gateway for availability; only running sends my file.</span></label>
          <button className="text-button share-button" onClick={() => copy(`${window.location.origin}/demo/index.html#sandbox`, 'Public demo link copied. No files, results, or tokens are included.')}><Icon name="link" size={14} /> Share playground</button>
          <div className="console-feedback" aria-live="polite" aria-atomic="true">{running ? 'Uploading and processing your document. Canceling stops the browser request; server processing may already have started.' : phase === 'loading' ? 'Checking model gateway availability…' : notice || (result ? 'Extraction complete. Output is the gateway response, not a simulated result.' : '')}</div>
          {(error || modelError) && <div className="error-message" role="alert">{error || modelError}{modelError && consent && <button className="text-button" onClick={() => { setPhase('loading'); setModelError(''); setModelAttempt(value => value + 1) }}>Retry model discovery</button>}</div>}
        </div>
        <div className="sandbox-footnote"><span>YOUR DOCUMENTS. YOUR DECISION.</span><p>This sandbox uses an optional cloud gateway. With your local server running, <a href={OFFLINE_STUDIO} title={LOCAL_STUDIO_NOTE}>open offline Studio on port 8000 <span aria-hidden="true">↗</span></a>.</p></div>
      </section>

      <section className="principles-section" id="architecture" aria-labelledby="architecture-title">
        <div className="section-heading"><div><span className="section-index">02 / BUILT DIFFERENT</span><h2 id="architecture-title">A workbench. Not a black box.</h2></div><p className="section-description">Keep your pipeline modular.<br />Keep your evidence close.</p></div>
        <div className="principle-grid">
          <article className="principle"><div className="feature-icon"><Icon name="layers" size={25} /><span>01</span></div><h3>Swap the engine.<br />Keep the workflow.</h3><p>A modular offline harness separates document handling, recognition, and review. TrOCR is the current adapter—not a promise that every model is plug-and-play.</p><div className="feature-bottom">ADAPTER-DRIVEN ARCHITECTURE <Icon size={15} /></div></article>
          <article className="principle"><div className="feature-icon"><Icon name="shield" size={25} /><span>02</span></div><h3>Local by default.<br />Cloud by choice.</h3><p>The offline studio keeps your workflow on your own machine. This separate demo sends a document only after explicit consent and a click on Run extraction.</p><div className="feature-bottom">EXPLICIT DATA BOUNDARIES <Icon size={15} /></div></article>
          <article className="principle"><div className="feature-icon"><Icon name="terminal" size={25} /><span>03</span></div><h3>Inspect the output.<br />Own the next step.</h3><p>Review extracted text, structured data, and region evidence when supplied by the gateway. Export a readable JSON or XML representation, without invented detections.</p><div className="feature-bottom">STRUCTURED, NOT OBSCURED <Icon size={15} /></div></article>
        </div>
        <div className="model-roster"><span className="technical-label">MODEL LANDSCAPE</span><span>TrOCR <small>CURRENT ADAPTER</small></span><span>LightOnOCR 3 <small>KNOWN IDENTITY · ADAPTER PENDING</small></span><span>Xiaomi OCR 0 <small>KNOWN IDENTITY · ADAPTER UNVERIFIED</small></span><p>Known identities: lightonai/LightOnOCR-3-0.8B, -1B, -4B; SeerRay-Lab/Xiaomi-OCR-0. Weights and adapters have not been tested here. No HF Space inference endpoint is verified; Xiaomi’s linked Space is a project page. Live availability comes only from the gateway.</p></div>
      </section>

      <section className="workbench-section" id="workbench" aria-labelledby="workbench-title">
        <div className="workbench-copy"><span className="section-index">03 / THE CONTROL ROOM</span><h2 id="workbench-title">Your documents.<br />Your machine.<br /><span>Your studio.</span></h2><p>Move from a single extraction to a deliberate review workflow. The local studio brings document regions, field evidence, and human review into one focused workspace.</p><a className="button primary" href={OFFLINE_STUDIO} title={LOCAL_STUDIO_NOTE}>Open local studio <Icon /></a><div className="release-actions"><button disabled aria-describedby="release-note"><Icon name="upload" size={14} /> Download desktop</button><button disabled aria-describedby="release-note"><Icon name="terminal" size={14} /> Install CLI</button></div><small id="release-note">No packaged desktop or CLI release is available.<br />Requires your local Studio server at 127.0.0.1:8000.</small></div>
        <div className="studio-mockup" aria-label="Illustrative studio concept, not a live dashboard">
          <div className="mockup-top"><div className="window-dots"><i /><i /><i /></div><span>ICR STUDIOS / WORKSPACE</span><span className="mockup-badge">ILLUSTRATIVE CONCEPT</span></div>
          <div className="mockup-body">
            <aside className="mockup-sidebar"><div className="mini-logo">icr<span> / STUDIO</span></div><div className="mockup-nav active"><Icon name="layers" size={13} /> Overview</div><div className="mockup-nav"><Icon name="file" size={13} /> Documents</div><div className="mockup-nav"><Icon name="check" size={13} /> Review queue</div><div className="mockup-nav"><Icon name="terminal" size={13} /> Adapters</div><span className="mockup-local"><span className="status-dot" /> LOCAL WORKSPACE</span></aside>
            <div className="mockup-content"><div className="mockup-title"><span>Workspace overview<small>Example data · no live activity</small></span><span className="panel-tag">DEMO WORKSPACE</span></div>
              <div className="mockup-stats"><div><small>DOCUMENTS</small><strong>24<span>sample files</span></strong></div><div><small>REVIEW QUEUE</small><strong>06<span>example tasks</span></strong></div><div><small>ADAPTER</small><strong className="adapter-value">TrOCR<span>local harness</span></strong></div></div>
              <div className="mockup-chart"><span>Example processing activity <small>ILLUSTRATIVE ONLY</small></span><svg viewBox="0 0 500 105" role="img" aria-label="Decorative example activity chart; not measured data"><path className="chart-grid" d="M0 20H500M0 50H500M0 80H500" /><path className="chart-area" d="M0 89 35 82 70 87 105 59 140 69 175 47 210 53 245 23 280 39 315 27 350 40 385 13 420 26 455 11 500 18V105H0Z" /><path className="chart-line" d="m0 89 35-7 35 5 35-28 35 10 35-22 35 6 35-30 35 16 35-12 35 13 35-27 35 13 35-15 45 7" /></svg><div className="chart-labels"><span>MON</span><span>TUE</span><span>WED</span><span>THU</span><span>FRI</span></div></div>
              <div className="mockup-table"><div><span>EXAMPLE DOCUMENT</span><span>WORKFLOW</span><span>STATE</span></div>{[['invoice_sample_01.pdf', 'Invoice', 'Reviewed'], ['intake_sample_02.png', 'Form', 'Needs review'], ['receipt_sample_03.jpg', 'Receipt', 'Reviewed']].map(row => <div key={row[0]}><span><Icon name="file" size={11} />{row[0]}</span><span>{row[1]}</span><span className={row[2] === 'Reviewed' ? 'reviewed' : ''}>{row[2]}</span></div>)}</div>
            </div>
          </div>
          <div className="mockup-footer"><span>CONCEPT PREVIEW / NOT A PRODUCT SCREENSHOT</span><span>NO TELEMETRY SHOWN</span></div>
        </div>
      </section>
      <section className="closing-section"><div className="eyebrow">LESS FRICTION. MORE STRUCTURE.</div><h2>Make your documents<br /><span>work for you.</span></h2><button className="button primary" onClick={tryOwn}>Explore the sandbox <Icon /></button><div className="closing-grid" aria-hidden="true" /></section>
    </main>
    <footer className="site-footer"><a className="footer-brand" href="/demo/index.html">ICR<span>STUDIOS</span></a><p>Privacy, plainly: previews stay in your browser. Enabling consent checks gateway models. Uploads leave your device only when you explicitly run extraction; the cloud gateway may retain or process them. Don’t upload sensitive documents.</p><div><a href={OFFLINE_STUDIO} title={LOCAL_STUDIO_NOTE}>Offline studio · local server <Icon size={13} /></a><span>BUILT FOR THE UNSTRUCTURED.</span></div></footer>
  </div>
}
