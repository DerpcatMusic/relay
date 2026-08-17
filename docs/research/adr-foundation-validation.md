# Foundational ADRs — Research and implementation evidence

## Scope
Validate the six Phase 0 ADRs for monorepo organization, a Rust portable core, the V1 WebRTC-compatible media wire, a 48 kHz RTP audio clock, Opus, and an acceptance-gated native transport bakeoff. This task changes documentation only.

## Acceptance criteria
- Six concise ADRs contain `Status`, `Context`, `Decision`, `Consequences`, and `Validation gates`.
- Truce remains a provisional plugin-shell choice; native transport implementations and service providers remain separately provisional.
- Standards claims are supported by no more than four targeted primary-source checks.
- Corrections and validation evidence are explicit.

## Sources consulted
Four targeted external primary-source checks were performed on 2026-08-15:

| Source | Evidence used |
| --- | --- |
| [RFC 8834 §§4.1–4.3](https://www.rfc-editor.org/rfc/rfc8834.html#section-4.1) | WebRTC media uses RTP/RTCP; §4.2 requires the full RTP/SAVPF profile and SRTP/SRTCP; payload mappings must be agreed before use. |
| [RFC 8827 §6.5](https://www.rfc-editor.org/rfc/rfc8827.html#section-6.5) | WebRTC implementations must support SRTP, DTLS, and DTLS-SRTP; media channels must use SRTP/SRTCP and offer DTLS-SRTP. |
| [RFC 7587 §§3.3, 4, 6.1](https://www.rfc-editor.org/rfc/rfc7587.html#section-3.3) | Defines Opus loss/FEC behavior and RTP payload mapping; RTP timestamps use 48 kHz for every Opus mode and sampling rate, and SDP advertises a 48 kHz RTP clock. |
| [RFC 6716 abstract, §§2.1.6–2.1.7](https://www.rfc-editor.org/rfc/rfc6716.html#section-2.1.6) | Defines Opus for interactive speech and music across a broad bitrate range and specifies packet-loss resilience and FEC controls. |

Repository context was checked separately in `docs/plans/2026-08-15-relay-master-plan.md` and `docs/plans/2026-08-15-relay-phase-0-foundation-plan.md`; these are internal requirements, not additional external primary-source checks.

## Findings
- Secure RTP/RTCP and DTLS-SRTP are standards-backed WebRTC requirements, so they can anchor the Relay-owned V1 conformance contract without selecting a native implementation or provider.
- The fixed 48 kHz value is specifically an Opus RTP timestamp clock. It is not a claim that device sampling, DSP processing, or every possible WebRTC audio codec must run at 48 kHz.
- Opus supports the intended interactive speech/music domain and exposes loss-resilience mechanisms, but exact packet duration, bitrate, channel, FEC, and complexity settings remain measurement decisions.
- The master plan identifies Truce as a plugin-shell candidate and native WebRTC libraries as transport candidates. They are distinct decision categories.
- The standards do not establish Relay-specific quality, tail latency, CPU, battery, cost, or operational fitness; the ADRs therefore leave those as acceptance gates.

## Explicit corrections
1. **Separated Truce from transport/provider candidates.** Early ADR wording incorrectly placed Truce in the native transport bakeoff. ADRs 0003, 0005, and 0006 now identify it only as a separate provisional plugin-shell choice.
2. **Narrowed the 48 kHz claim.** Replaced the overbroad phrase “Opus and WebRTC audio conventions” with the precise RFC 7587 rule for Opus RTP timestamps. Device rates and resampler choices remain open.
3. **Avoided an unsupported interoperability shortcut.** Replaced “broad WebRTC interoperability” as an automatic Opus consequence with the narrower, evidenced benefit of one standardized RTP payload format and test matrix. Actual interoperability is a validation gate.
4. **Scoped the WebRTC wire evidence.** The V1 ADR now cites standardized RTP/RTCP media security and Opus packetization; it does not treat Truce, signaling infrastructure, NAT traversal service, or a native library as part of the wire definition.

## Decisions applied
- `0001-monorepo.md`: one repository with enforced internal boundaries and atomically versioned contracts.
- `0002-rust-core.md`: portable Rust core with narrow FFI and no platform, plugin-shell, or provider types in its public API.
- `0003-webrtc-v1-wire.md`: WebRTC-compatible secure RTP/RTCP plus negotiated Opus, with Relay-owned conformance fixtures.
- `0004-48khz-network-clock.md`: 48,000-tick Opus RTP timestamp domain with explicit boundary conversion and drift handling.
- `0005-opus.md`: Opus required for V1 while profiles and implementation remain provisional and measured.
- `0006-native-transport-bakeoff.md`: no selection until common functional, quality, security, operational, cost, portability, and exit gates pass.

## Validation evidence
- All four RFC Editor requests returned HTTP 200 on 2026-08-15.
- A local structure check found all five required headings exactly once in each of the six ADRs; each ADR is 22–23 lines.
- All seven created Markdown files end with a newline and contain no tabs or trailing whitespace.
- `git diff --check -- docs/adr docs/research/adr-foundation-validation.md` reported no errors. The repository is an uncommitted foundation with all files currently untracked, so the target-file list was also verified explicitly with `git ls-files --others --exclude-standard`.
- No source-code file was created or edited by this task, and no commit was made.

## Deferred follow-ups
- Declare numeric quality, latency, resource, and cost thresholds before the transport bakeoff begins.
- Add candidate-specific evidence only in the bakeoff record or a superseding selection ADR.
- Evaluate Truce under its own plugin-shell ADR; do not fold that decision into native transport selection.
