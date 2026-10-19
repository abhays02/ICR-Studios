export const workflows = {
  'README.md': {
    label: 'From scan to traceable review',
    steps: [
      ['Import', 'Upload an upright scan or import a fictional fixture.'],
      ['Inspect', 'Check original-pixel regions; missing weights produce a failure, not invented text.'],
      ['Correct', 'Save explicit human corrections separately from model evidence.'],
      ['Export', 'Download a draft, or finalize only after every review gate passes.'],
    ],
  },
  'docs/STUDIO.md': {
    label: 'The operator’s review loop',
    steps: [
      ['Unlock', 'Use only your operator token; the browser keeps it in memory.'],
      ['Inspect', 'Confirm alignment and inspect the template’s original-pixel regions.'],
      ['Review', 'Save human corrections; new template versions require re-upload.'],
      ['Finalize', 'Review every field and resolve validation errors before final approval.'],
    ],
  },
  'docs/API.md': {
    label: 'An authenticated document journey',
    steps: [
      ['Authenticate', 'Send the operator bearer token with protected API requests.'],
      ['Upload', 'Submit a supported file matching the selected template dimensions.'],
      ['Observe', 'Queue a job and read authenticated SSE progress and failure evidence.'],
      ['Review & export', 'Save corrections, complete review gates, then request the appropriate export.'],
    ],
  },
  'docs/CONFIGURATION.md': {
    label: 'Configure, start, verify',
    steps: [
      ['Choose', 'Select a local runtime profile; inference dependencies and weights are separate.'],
      ['Protect', 'Initialize private keys and operator tokens once; never publish private state.'],
      ['Start', 'Run the source helper or nonroot CPU container on loopback.'],
      ['Verify', 'Check health and readiness; readiness is not proof of successful OCR.'],
    ],
  },
  'docs/CONFIG_REFERENCE.md': {
    label: 'Resolve effective settings',
    steps: [
      ['Defaults', 'Begin with the schema’s actual defaults and constraints.'],
      ['Profile', 'Apply the selected YAML; unknown keys fail closed.'],
      ['Overrides', 'Permitted persisted runtime values precede environment and explicit startup settings.'],
      ['Snapshot', 'New jobs snapshot effective settings; existing jobs keep their original snapshot.'],
    ],
  },
  'docs/DESIGN.md': {
    label: 'Local evidence, explicit human authority',
    steps: [
      ['Browser', 'Studio sends same-origin authenticated requests.'],
      ['Storage', 'Private local storage retains encrypted images and sensitive rows.'],
      ['Worker', 'A durable queue runs bounded local attempts without remote fallback.'],
      ['Human review', 'Corrections remain distinct from raw evidence; approval is explicit.'],
    ],
  },
  'docs/MODELS.md': {
    label: 'Model availability is not model validation',
    steps: [
      ['Review', 'Check adapter support, artifact provenance and license approval.'],
      ['Provision', 'Explicitly fetch approved immutable artifacts outside service startup.'],
      ['Verify offline', 'Verify the inventory; file presence alone does not prove inference.'],
      ['Evaluate', 'Test the actual runtime and accuracy before making capability claims.'],
    ],
  },
  'docs/EVALUATION.md': {
    label: 'Evidence before accuracy claims',
    steps: [
      ['Separate', 'Keep synthetic plumbing checks distinct from real handwriting evaluation.'],
      ['Curate', 'Prepare authorized, representative data and verified ground truth.'],
      ['Measure', 'Evaluate recognition, validation and review outcomes under a recorded setup.'],
      ['Report', 'Publish measured evidence and limitations; no current accuracy claim is established.'],
    ],
  },
  'docs/STATUS.md': {
    label: 'Read the evidence ladder',
    steps: [
      ['Implemented', 'Identify code paths with concrete implementation evidence.'],
      ['Verified', 'Distinguish exercised manual-review flows from adapter availability.'],
      ['Open gates', 'Real weights, handwriting accuracy and hardware execution remain separate gates.'],
      ['Scope', 'Do not infer production readiness, compliance or automatic approval.'],
    ],
  },
  'SECURITY.md': {
    label: 'Keep the public and private boundaries separate',
    steps: [
      ['Limit exposure', 'Use local processing and protected ingress for external deployments.'],
      ['Protect state', 'Keep keys, tokens and encrypted runtime storage private.'],
      ['Control exports', 'Downloaded JSON/CSV is outside application encryption.'],
      ['Respond', 'Follow the security policy for incidents, recovery and disclosure.'],
    ],
  },
  'data/samples/README.md': {
    label: 'Synthetic-only demonstrations',
    steps: [
      ['Identify', 'Fixtures contain fictional script-font simulations, not real handwriting.'],
      ['Import', 'Use one of the five importable fixtures; the skewed sixth is blocked.'],
      ['Inspect', 'Exercise regions and human corrections without claiming OCR accuracy.'],
      ['Capture', 'Publish only reviewed synthetic visuals, never private banking data or tokens.'],
    ],
  },
  'studio/README.md': {
    label: 'Two frontends, separate boundaries',
    steps: [
      ['Choose', 'The offline workspace and the optional public showcase have separate builds and runtimes.'],
      ['Start', 'Use the documented local workspace or isolated showcase gateway instructions.'],
      ['Consent', 'Showcase live mode sends uploads off-device only after explicit consent and requires a configured compatible Space.'],
      ['Verify', 'Use existing verification commands and synthetic data; illustrative output is not measured model performance.'],
    ],
  },
};
