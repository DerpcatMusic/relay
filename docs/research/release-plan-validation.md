# Release Plan Validation

## Scope

Focused validation of `docs/plans/2026-08-15-relay-release-plan.md`. This review is limited to release tooling, platform signing/notarization, SBOM generation, and artifact provenance. The master plan and code are not modified.

## Validation Criteria

- Release commands and tool names match current official documentation.
- Platform signing/notarization steps identify the required trust material and verification path.
- SBOM and provenance recommendations use maintained, interoperable standards and official tooling.
- Any unsupported, ambiguous, or stale plan statement is called out as an explicit potential correction.

## Primary Source Table

| # | Primary source | Area | Plan claim checked | Evidence status |
|---|---|---|---|---|
| 1 | [Cargo unstable `sbom` reference](https://doc.rust-lang.org/cargo/reference/unstable.html#sbom) | Cargo SBOM inputs | Whether Cargo directly emits the planned SPDX/CycloneDX artifact SBOMs | **Qualified:** Cargo emits per-artifact JSON *precursor* files, not finished SPDX/CycloneDX documents, and the feature is unstable (`-Z sbom`). |
| 2 | [Apple: Customizing the notarization workflow](https://developer.apple.com/documentation/security/customizing-the-notarization-workflow) | macOS notarization | `notarytool`, waiting, stapling, artifact coverage, and verification assumptions | **Mostly supported, with packaging qualifications:** use `notarytool`; ZIPs and standalone binaries cannot be stapled; custom third-party installers may require payload and installer notarization rounds. |
| 3 | [Microsoft: SignTool](https://learn.microsoft.com/en-us/windows/win32/seccrypto/signtool) | Windows Authenticode | Signing, timestamp digest, verification policy, and CI failure semantics | **Supported but under-specified:** SHA-256 file/timestamp digests should be explicit; verification should use Authenticode policy and require a timestamp; warnings have a distinct exit code and must fail release qualification. |
| 4 | [SLSA provenance v1.1 specification](https://slsa.dev/spec/v1.1/provenance) | Provenance | Required subject/input/builder binding and final-artifact verification | **Qualified:** SLSA provenance identifies outputs as attestation subjects and records build definition, resolved dependencies, and builder identity; the consulted v1.1 page is marked retired, while its stable predicate type is `https://slsa.dev/provenance/v1`. |

## Overall Result

**Conditionally validated.** The plan’s release policy is strong and its deferred tasks (REL-02 through REL-05) are appropriately separated, but automation should not begin until the four corrections below are incorporated into those task specifications. The only plan-level correctness risk is the ordering of provenance before byte-changing signing/notarization operations; without a final-subject or chained-attestation design, step 6 cannot prove provenance for the downloaded bytes. The other corrections make already-correct policy executable and fail-closed.

## Findings

### 1. Cargo SBOM input is useful but not a complete release SBOM (qualified)

Cargo’s official reference documents an unstable `-Z sbom` facility that emits `<artifact>.cargo-sbom.json` precursor files for executable/linkable outputs. The precursors record crates, dependency edges/kinds, enabled features, target information, and compiler identity. Cargo explicitly says an SBOM tool must incorporate these precursors; they are not themselves SPDX or CycloneDX release SBOMs.

**Plan impact:** The plan is directionally correct to defer format/tool selection to REL-02 and to require bundled runtime dependencies. REL-02 must not equate Cargo’s precursor JSON with the required SPDX/CycloneDX deliverable. If the release toolchain must remain on stable/MSRV Rust, the implementation cannot require Cargo’s currently unstable `-Z sbom` feature without an explicit nightly-tool exception.


### 2. macOS flow is correct at policy level but needs artifact-specific sequencing (mostly supported)

Apple’s official workflow uses `xcrun notarytool submit … --wait`, recommends checking the notarization log even after acceptance, and recommends stapling so Gatekeeper can validate without network access. Apple accepts ZIP archives for submission but does **not** allow tickets to be stapled to ZIPs; the contained app/bundle must be stapled and then re-archived. Tickets also cannot currently be stapled to standalone binaries. Apple further says custom third-party installers need two rounds: notarize/staple installed payloads, package them, then notarize the installer.

**Plan impact:** Steps 5–6 and REL-05 are sound as policy. The artifact manifest/checksums must describe the final post-staple/repackaging bytes, not merely the initially submitted archive. REL-05 needs an explicit sequence per plugin bundle, PKG, DMG, ZIP, and any custom installer; it should retain and inspect the notarization log, not only the accepted status.


### 3. Windows signing policy is correct but exact verification/failure semantics matter (mostly supported)

Microsoft documents `signtool sign`, RFC 3161 timestamping via `/tr`, and signature verification. Current SDK-era SignTool requires explicit file and timestamp digest algorithms (`/fd` and `/td`) and recommends SHA-256. For ordinary Authenticode verification, `/pa` selects the Default Authentication Verification Policy; `/tw` warns when a timestamp is absent. SignTool returns `0` for success, `1` for failure, and `2` for warnings. The signing command documentation also notes that timestamp failure can be a warning, so merely checking for a nonfailure message or treating warnings as success would violate the plan’s mandatory timestamp policy.

**Plan impact:** Step 4 and REL-04 are directionally correct. REL-04 should pin the SDK/SignTool version and exact algorithms, use an RFC 3161 timestamp service, verify under Authenticode policy, require a timestamp, and treat exit code `2`/warnings as a release failure. Verification on a clean machine remains valuable for trust-chain behavior.


### 4. Provenance content is well aimed, but ordering currently risks attesting the wrong bytes (correction needed)

The SLSA provenance model is an in-toto attestation whose `subject` identifies the produced artifact(s). Its build definition captures the build type and external parameters, may capture resolved dependencies such as the exact Git commit, and its run details identify the builder and invocation. This supports the plan’s intent to bind source, workflow/build identity, inputs, and output digests.

However, the plan currently says to attest in step 3, then Authenticode-sign in step 4, then notarize/staple/repackage in step 5. Those later operations change release bytes. A provenance subject digest emitted before them cannot also identify the final downloadable artifact verified in step 6. The plan needs either (a) final provenance whose subject is the post-signing/post-stapling/post-repackaging artifact, produced within a carefully defined trusted release build, or (b) a verifiable chain of attestations linking the initial build output through signing/notarization/packaging transformations to the final release digest. Reordering metadata publication is not a rebuild, but the trust boundary and builder identity must be explicit.

The consulted SLSA v1.1 page is itself marked **Retired**. It says the predicate type is the major-version URI `https://slsa.dev/provenance/v1`, which resolves to the latest minor specification. REL-03 should therefore pin an interoperable predicate/schema and verifier deliberately, rather than copy the retired documentation URL or invent an ad hoc provenance object.

**Plan impact:** REL-03 is necessary and correctly demands a consumer verification command, but its sample must verify the exact final downloadable digest (or a complete transformation chain), expected builder identity/build type, source commit, and external parameters—not merely validate a signature cryptographically.

## Explicit Potential Corrections to the Master Plan

1. In REL-02 or its future implementation specification, state that Cargo `-Z sbom` output is optional precursor/input data, not the final SPDX/CycloneDX SBOM. Record whether a nightly Cargo exception is acceptable; otherwise select a stable generator and prove artifact-level coverage, including non-Rust bundled content.
2. Clarify in the notarization flow that final release digests/manifests are computed **after** any stapling and repackaging. REL-05 should state which artifact is submitted, which item is stapled, and whether a custom installer requires payload-first and installer-second notarization. Add notarization-log review to acceptance evidence.
3. Expand REL-04 acceptance to require explicit SHA-256 file and RFC 3161 timestamp digests, Authenticode-policy verification with timestamp presence, a pinned SignTool/SDK version, and failure on warnings/exit code `2` (including timestamp warnings).
4. Fix the attest/sign/notarize order: require provenance to name the final post-signing/post-stapling/repackaging download digest, or define and verify a chained transformation-attestation model from build output to final artifact. REL-03 must test identity/policy expectations in addition to signature validity and should use the stable SLSA provenance v1 predicate URI with an explicitly selected current schema/tool.

## Decisions Reflected in the Plan

- Keep SBOM format/tool selection as a separate executable task (REL-02).
- Require one SBOM per artifact/package plus an aggregate release SBOM.
- Include bundled runtime dependencies rather than limiting inventory to Rust package resolution.
- Use Apple notarization as a blocking gate, staple where the artifact supports it, and require clean-machine verification.
- Keep exact macOS identity/entitlement/notarize/staple/verify sequences in REL-05 rather than pretending one command fits every artifact type.
- Require Authenticode signing **and** timestamping for Windows deliverables, with a clean-machine verification procedure owned by REL-04.
- Bind provenance to source commit, build/workflow identity, inputs, and artifact digest; publish it beside artifacts and provide a consumer verification command (REL-03).
- Preserve build-once/promotion semantics: trust finalization and metadata generation may transform/package the candidate, but stable promotion must reuse the already-qualified final bytes.

## Validation Boundaries

- Four targeted official sources were used; no broad ecosystem/tool bakeoff was performed.
- This validation does not select the eventual SBOM generator, provenance generator/verifier, certificate provider, timestamp service, or CI platform.
- Commands were validated as documented behavior, not executed against real signing identities or release artifacts; REL-02 through REL-05 still require sample-artifact dry runs.

## Validation Proof

- Evidence file created before inspecting the plan or consulting external sources, as required.
- Validation completed at `2026-08-15T22:15Z`.
- Master-plan SHA-256 observed after validation: `0547ef24fb5fa31256ef71a630d3b4401bb5ab2dd0f1bbdadd9228ef7c92c312`.
- `git diff -- docs/plans/2026-08-15-relay-release-plan.md` produced no diff.
- Master plan modified: **No**.
- Code modified: **No**.
- Primary sources consulted: **4 / 4 maximum; research stopped at the requested cap**.
- Source 1 retrieved successfully (HTTP 200) and its findings were recorded before consulting another source.
- Source 2 and its official Markdown representation were retrieved successfully (HTTP 200); findings were recorded before consulting another source.
- Source 3 was retrieved successfully (HTTP 200); findings were recorded before consulting another source.
- Source 4 was retrieved successfully (HTTP 200); its retired status and normative predicate URI were recorded. No further primary sources were consulted.
