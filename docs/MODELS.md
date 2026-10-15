# Model evidence and deployment decisions

Research date: **2026-10-15 UTC**. This is a design-phase evidence register, not an inference benchmark or a statement that weights have been installed. **No weights were downloaded.** Code licenses, weight licenses, and dataset licenses are separate.

## Evidence quality and network limitations

Direct HTTPS requests to Hugging Face failed DNS resolution (`No address associated with hostname`) for **all seven** model metadata endpoints below. A second path through the web-fetch tool also failed for the AIDC and Microsoft metadata endpoints and the Microsoft raw card. The OpenRouter website likewise failed DNS resolution. Official GitHub retrieval succeeded and was used instead.

Consequently:

- No HF model commit SHA, current weight license, access status, or exact tensor files were directly verified. `unknown` is intentional, not permission to use `main`.
- DNS failure is **not** evidence of a missing model, a 404, a rename, or a gated repository. In particular, do not state that Riksarkivet is missing.
- A supplementary web search returned inconsistent citations: an AIDC **OvisOCR** page was cited for **OvisOCR2**, and an ATH discussion page was cited as an inference card. Its proposed abbreviated SHA and license/interface claims were **not accepted as verified evidence**.
- GitHub commit SHAs below pin **code or documentation**, never HF weights. A Git blob SHA is also not a model revision.

### HF requests attempted, all unsuccessful

| Requested exact ID | Official metadata URL | Card URL to retrieve when connectivity returns | Weight revision |
| --- | --- | --- | --- |
| `AIDC-AI/OvisOCR2` | https://huggingface.co/api/models/AIDC-AI/OvisOCR2 | https://huggingface.co/AIDC-AI/OvisOCR2 | unknown |
| `ATH-MaaS/OvisOCR2` | https://huggingface.co/api/models/ATH-MaaS/OvisOCR2 | https://huggingface.co/ATH-MaaS/OvisOCR2 | unknown |
| `datalab-to/chandra-ocr-2` | https://huggingface.co/api/models/datalab-to/chandra-ocr-2 | https://huggingface.co/datalab-to/chandra-ocr-2 | unknown |
| `ucaslcl/GOT-OCR2_0` | https://huggingface.co/api/models/ucaslcl/GOT-OCR2_0 | https://huggingface.co/ucaslcl/GOT-OCR2_0 | unknown |
| `VikParuchuri/surya` | https://huggingface.co/api/models/VikParuchuri/surya | https://huggingface.co/VikParuchuri/surya | unknown |
| `Riksarkivet/trocr-base-handwritten` | https://huggingface.co/api/models/Riksarkivet/trocr-base-handwritten | https://huggingface.co/Riksarkivet/trocr-base-handwritten | unknown |
| `microsoft/trocr-base-handwritten` | https://huggingface.co/api/models/microsoft/trocr-base-handwritten | https://huggingface.co/microsoft/trocr-base-handwritten | unknown |

Additional unsuccessful request: https://huggingface.co/microsoft/trocr-base-handwritten/raw/main/README.md.

## Verified upstream sources

The following URLs were retrieved through the official GitHub contents/commit API on the research date. The linked immutable URLs identify the retrieved content at its resolved repository revision.

| Source | Verified code/document revision | Finding |
| --- | --- | --- |
| [HF Transformers TrOCR documentation](https://github.com/huggingface/transformers/blob/052e652d6d53c2b26ffde87e039b723949a53493/docs/source/en/model_doc/trocr.md) | `052e652d6d53c2b26ffde87e039b723949a53493`, tag `v4.46.3` | Explicit working interface for Microsoft handwriting checkpoint, shown below. Documentation Apache-2.0; not a weight-license determination. |
| [Microsoft TrOCR README](https://github.com/microsoft/unilm/blob/6f60612e7cc86a2a1ae85c47231507a587ab4e01/trocr/README.md), [root license](https://github.com/microsoft/unilm/blob/6f60612e7cc86a2a1ae85c47231507a587ab4e01/LICENSE) | `6f60612e7cc86a2a1ae85c47231507a587ab4e01` | Original fairseq implementation, base 334M parameters, IAM handwriting task; root source license MIT. Does not establish the current HF weight license. |
| [Ovis README](https://github.com/ATH-MaaS/Ovis/blob/e1916bf3cb0ca123e71ab173ff4463df5c3a71b5/README.md), [docs directory](https://github.com/ATH-MaaS/Ovis/tree/e1916bf3cb0ca123e71ab173ff4463df5c3a71b5/docs) | `e1916bf3cb0ca123e71ab173ff4463df5c3a71b5` | `AIDC-AI/Ovis` commit lookup resolves to `ATH-MaaS/Ovis`. Contains `docs/OvisOCR2_Tech_Report.pdf` (directory listing only; paper not read). README inference examples are **Ovis2.5**, not OvisOCR2. Source README states Apache-2.0. |
| [Chandra README](https://github.com/datalab-to/chandra/blob/d4f7467435aa4137d9539f000ddf0b7ced3eb43f/README.md), [manifest](https://github.com/datalab-to/chandra/blob/d4f7467435aa4137d9539f000ddf0b7ced3eb43f/pyproject.toml), [model license](https://github.com/datalab-to/chandra/blob/d4f7467435aa4137d9539f000ddf0b7ced3eb43f/MODEL_LICENSE) | `d4f7467435aa4137d9539f000ddf0b7ced3eb43f` | Explicit checkpoint `datalab-to/chandra-ocr-2`; package version `0.2.0`; Apache-2.0 code, modified OpenRAIL-M model terms. |
| [GOT README](https://github.com/Ucas-HaoranWei/GOT-OCR2.0/blob/179ed086ad6bac0908a04ee06b3fc382021aa566/README.md), [inference source](https://github.com/Ucas-HaoranWei/GOT-OCR2.0/blob/179ed086ad6bac0908a04ee06b3fc382021aa566/GOT-OCR-2.0-master/GOT/demo/run_ocr_2.0.py) | `179ed086ad6bac0908a04ee06b3fc382021aa566` | Exact requested HF ID linked by README; official local CLI and concrete `GOTQwenForCausalLM` generation implementation. |
| [Surya recognition source](https://github.com/datalab-to/surya/blob/a2363d3311773a2b6145a40458043211c50c52f4/surya/recognition/__init__.py), [manifest](https://github.com/datalab-to/surya/blob/a2363d3311773a2b6145a40458043211c50c52f4/pyproject.toml), [code license](https://github.com/datalab-to/surya/blob/a2363d3311773a2b6145a40458043211c50c52f4/LICENSE), [model license](https://github.com/datalab-to/surya/blob/a2363d3311773a2b6145a40458043211c50c52f4/MODEL_LICENSE) | `a2363d3311773a2b6145a40458043211c50c52f4` | Package `surya-ocr` version `0.22.1`; present-day interface differs from older Surya tutorials. Apache-2.0 code, modified OpenRAIL-M model terms. Does not prove what the requested legacy HF ID contains. |

Retrieval API forms were `https://api.github.com/repos/{owner}/{repo}/contents/{path}?ref={revision}` and `https://api.github.com/repos/{owner}/{repo}/commits/{ref}`. Initial unpinned README requests were subsequently tied to resolved commits. The Surya README response exceeded the tool's inline limit; findings here rely on the explicitly retrieved source, manifest, and licenses instead.

## Candidate decisions

### OvisOCR2: namespace is not an interchangeable alias

Both requested HF IDs remain unverified. The official **GitHub** repository move is evidenced; an equivalent **HF** move is not. Never silently substitute `ATH-MaaS/OvisOCR2` for `AIDC-AI/OvisOCR2`, or OvisOCR for OvisOCR2.

The retrieved Ovis README uses Ovis2.5 `vllm serve ... --trust-remote-code`, Torch 2.4.0 / Transformers 4.51.3, and an Ovis2.5 Python example. These do **not** establish OvisOCR2 requirements or APIs. No verified OvisOCR2 VRAM minimum or executable adapter is available from the retrieved files. Keep both IDs in the candidate register, not the active backend list. Revisit after the exact card, config, custom code, license, and complete weight SHA are retrieved and reviewed.

### Chandra OCR 2

Verified upstream interface: `chandra input.pdf ./output --method hf`, or `chandra_vllm` followed by the CLI using its vLLM method. `MODEL_CHECKPOINT=datalab-to/chandra-ocr-2` is explicitly documented. Outputs are Markdown, HTML, and metadata JSON, not the application's banking schema. HTML must be treated as untrusted.

HF extras at the pinned code revision require Torch `>=2.8.0`, torchvision `>=0.23.0`, Transformers `>=5.2.0`, accelerate `>=1.11.0`; these are upstream ranges, **not an installation lock**. Isolate from baseline inference dependencies. Upstream throughput is measured on **NVIDIA H100 80GB**, vLLM, 96 concurrent sequences: 1.44 pages/s, average 60s, p95 156s. That is an upstream benchmark configuration, **not a minimum or our measured performance**.

The retrieved modified model license restricts commercial use above **$2M prior-year gross revenue or total funding**, and competitive services; it also has attribution, use-restriction, and share-alike/output provisions. Bank deployment requires legal review or an appropriate commercial agreement. Do not label the weights simply “Apache” or “commercially unrestricted.” Default disabled.

### GOT-OCR2.0

Verified official CLI from the upstream project directory:

`python GOT/demo/run_ocr_2.0.py --model-name /approved/local/snapshot --image-file /local/crop.png --type ocr`

The retrieved source loads `AutoTokenizer` with `trust_remote_code=True`, uses `GOTQwenForCausalLM.from_pretrained(..., device_map='cuda', use_safetensors=True, pad_token_id=151643).eval()`, then CUDA bfloat16 and `model.generate(...)`. It uses 1024-pixel image processors and 4096 maximum new tokens. README reports its environment as CUDA 11.8 + Torch 2.0.1 and Flash-Attention; no minimum VRAM verified.

Do not invent a `.chat` API for this snapshot. A different HF-converted ID is linked upstream (`stepfun-ai/GOT-OCR-2.0-hf`) but is not identical to the requested ID. The README's code/data license badges link to another project's license files; they are insufficient to establish the precise requested checkpoint's rights. Weight license unresolved. The demo also evaluates `--box` and fetches arbitrary image URLs: **never expose those behaviors**. Any future adapter must use local sanitized crops, parsed numeric boxes, a reviewed pinned runtime, no rendering, and a hard subprocess deadline. Default disabled.

### Surya

Do not install a guessed model via `AutoModel.from_pretrained("VikParuchuri/surya")`. The requested HF ID's contents, license and revision remain unknown.

Verified current source interface:

- `from surya.recognition import RecognitionPredictor`
- `RecognitionPredictor()(images)` performs full-page recognition.
- `RecognitionPredictor()(images, layout_results, full_page=False)` performs block recognition, returning `PageOCRResult` with `blocks`, polygons, HTML, and error/skipped indicators.

At this code revision the constructor accepts an optional inference **manager**, not the older `FoundationPredictor` examples. Package manifest requires Transformers `>=5.12.1` and Torch `>=2.7.0,<3`; do not mix this into the historical baseline environment without a new compatibility run. No hardware minimum verified.

Critical confidence caveat: retrieved recognition code substitutes **1.0** when `mean_token_prob` is absent, and uses 1.0 for skipped blocks. The application must map unavailable evidence to **null**, not present these values as certainty. Block/full-page token averages are not field correctness probabilities.

The source model license has a **$5M**, not Chandra's $2M, revenue/funding threshold, competition restrictions, attribution and share-alike/output provisions. This does not resolve the license of the requested legacy HF checkpoint. Default disabled pending exact artifact and legal review.

### Riksarkivet and Microsoft handwriting fallback

Riksarkivet availability and specialization could not be verified; do not assert it is Swedish, licensed Apache, or absent based on its name.

**Recommended first executable inference implementation: Microsoft TrOCR on explicit single-line field crops, loaded from an operator-provisioned local snapshot.** This is an evidence-based fallback choice, not a confirmed missing-Riksarkivet substitution. HF's own retrieved documentation explicitly demonstrates:

1. `TrOCRProcessor.from_pretrained("microsoft/trocr-base-handwritten")`.
2. `VisionEncoderDecoderModel.from_pretrained("microsoft/trocr-base-handwritten")`.
3. RGB PIL image → `processor(image, return_tensors="pt").pixel_values`.
4. `model.generate(pixel_values)` → `processor.batch_decode(ids, skip_special_tokens=True)[0]`.

Implementation should use the **same classes** with a validated local directory and `local_files_only=True`, `trust_remote_code=False`, model `eval()`, `torch.inference_mode()`, CPU float32 first, greedy bounded generation, and controlled worker concurrency. Do not implicitly fetch weights at startup. Reject an incomplete snapshot as `model_unavailable`, not fabricated OCR.

**Exact reference interface pin:** Transformers documentation tag `v4.46.3`, commit `052e652d6d53c2b26ffde87e039b723949a53493`. This is a verified historical interface reference, **not a recommendation to deploy an old unreviewed dependency version**. BACKEND must select security-reviewed compatible package versions and lock them after targeted tests. No Torch package version or HF weight revision is approved by this phase.

For artifacts, require a manifest with exact HF ID, complete 40-hex upstream SHA once available, file SHA-256 inventory, model/processor configuration, license evidence and operator approval. For an already-local snapshot without HF provenance, use an explicit local artifact ID and file digests; never mislabel a local digest as an HF SHA. Promotion requires provenance review.

Original Microsoft source identifies TrOCR-base as **334M parameters**. Float32 parameters alone are approximately 1.34GB decimal; actual process RAM additionally includes framework, activations, decoding and image tensors. **4–8GB process RAM is an engineering trial budget, not a verified requirement.** CPU support follows the documented standard Transformers interface; no bank-domain CPU/GPU latency, memory or accuracy was measured. Do not advertise realtime.

TrOCR is not a detector or form parser. A multi-line address needs explicit per-line ROIs or a later verified detector; arbitrary whole-page input is out of scope for this baseline. IAM performance does not predict bank-form accuracy. Dates, checkboxes, signatures and numeric identifiers need separate validation or human review. No signature verification, identity verification, lending decision or payment instruction may be inferred.

## OpenRouter: latest retrieved official contract

Official website attempt (failed DNS): https://openrouter.ai/docs/api/reference/overview.

Official docs repository retrieved at **`8eded4ebd73d79f1152cd28acc69ff4c97b9a51d`**, committed **2026-10-15 22:22:37Z**, sourced by upstream from `openrouter-web@7524d0712abe055a0dae93a1cb250d22b62f05c1`. This is the latest retrieved snapshot, not a guarantee of future wire behavior.

Successfully retrieved source URLs:

- https://github.com/OpenRouterTeam/docs/blob/8eded4ebd73d79f1152cd28acc69ff4c97b9a51d/quickstart.mdx
- https://github.com/OpenRouterTeam/docs/blob/8eded4ebd73d79f1152cd28acc69ff4c97b9a51d/api_reference/authentication.mdx
- https://github.com/OpenRouterTeam/docs/blob/8eded4ebd73d79f1152cd28acc69ff4c97b9a51d/guides/overview/models.mdx
- https://github.com/OpenRouterTeam/docs/blob/8eded4ebd73d79f1152cd28acc69ff4c97b9a51d/guides/overview/multimodal/image-understanding.mdx
- https://github.com/OpenRouterTeam/docs/blob/8eded4ebd73d79f1152cd28acc69ff4c97b9a51d/guides/features/structured-outputs.mdx
- https://github.com/OpenRouterTeam/docs/blob/8eded4ebd73d79f1152cd28acc69ff4c97b9a51d/guides/features/zdr.mdx
- https://github.com/OpenRouterTeam/python-sdk/blob/81812ec3af15d6b70585c1bac288e517443ae382/src/openrouter/components/security.py

SDK commit `81812ec3af15d6b70585c1bac288e517443ae382` identifies HTTP bearer security on `Authorization`; tool output redacted authentication examples in the docs, so bearer syntax was independently checked in this source. Error documentation was retrieved but too large for inline inspection; no detailed current status-code guarantees are inferred from it.

### Contract to implement and test, only if explicitly enabled

- `POST https://openrouter.ai/api/v1/chat/completions`, JSON with `Content-Type: application/json`. The `Authorization` header uses the HTTP bearer scheme and the server-held API key. Optional attribution headers are currently `HTTP-Referer` and **`X-OpenRouter-Title`**. No browser-held provider key.
- Explicit configured model ID, `messages`, `stream:false`. Do not hardcode a “latest” alias or a supposed always-free model.
- Image content uses `messages[].content` parts: text first (`type:"text", text:...`), then `type:"image_url", image_url:{url:"data:image/png;base64,..."}`. Never provide an external URL to a bank image. Docs list PNG, JPEG, WebP and GIF; app initially restricts to sanitized PNG/JPEG.
- Discover model metadata from `GET https://openrouter.ai/api/v1/models`; require `architecture.input_modalities` to contain `image` for vision and text output. Catalog supports opt-in pagination (`offset`, `limit`, `links.next`); validate any next URL remains on the fixed origin. Use metadata only, not prompts, for discovery.
- `response_format:{type:"json_schema",json_schema:{name,strict:true,schema}}` for supported endpoints; `provider.require_parameters:true`. Strict mode is **not universally guaranteed**; independently validate every returned field with the same local schema. No tool execution, plugins, browsing or model-supplied URLs.
- `provider.zdr:true`; fixed provider allowlist and disabled provider fallback are recommended policy, but the precise allowlist/fallback parameter names require checking the provider-routing contract before implementation. ZDR applies to inference routing, not optional tools/plugins. Upstream permits implicit in-memory prompt caches even with ZDR. It is a vendor policy, not proof of regulatory compliance or absolute absence of processing.
- Nonstreaming response is read from `choices[0].message.content`; empty choices, refusals, error envelopes, non-JSON output, truncation and schema-invalid content become typed failure/review states. Keep returned model/request ID and usage when supplied; do not assume token logprobs or accuracy confidence are available.
- Enforce connection/read/total deadlines, bounded response bytes, output tokens and cost. Do not automatically retry a request that may have been accepted and billed after a read timeout; record `outcome_unknown`.
- Record consent, field IDs, crop digest, endpoint, resolved model when supplied, policy/config generation and attempt ID **before** sending. Refusal of consent or remote failure never triggers another remote provider silently.

No live catalog or paid inference call was made. A contract test against mock recorded-shape responses is possible offline; a consented synthetic-image integration check remains required before enabling this adapter.

## Promotion gate for any model

1. Retrieve `GET /api/models/{id}`, record exact ID, redirects and full SHA; retrieve README, LICENSE, config and processor files **at that SHA**. A 404 and a network error have distinct statuses.
2. Review code, weights and dataset terms separately; verify custom code before allowing it. Prefer safetensors; no arbitrary remote code execution inside the web process.
3. Produce a security-reviewed package lock and a manifest/checksum inventory; fetch weights only in an explicit administrator provisioning action with a bounded download ledger.
4. Run a real single-line smoke test on approved synthetic data, followed by held-out handwritten evaluation; measure peak RAM/VRAM, warm/cold latency and failure behavior on the actual target hardware.
5. Keep candidate adapters absent/disabled until implemented. Do not ship generic wrappers that invent compatible APIs. Missing weights must not be replaced with ground truth or sample text.
