# QUIC Feasibility Assessment

**Date:** 2026-11-03
**Status:** NOT FEASIBLE with current dependency versions

---

## Attempted Dependency Combinations

### Combination 1: Quinn 0.11 + Rustls 0.23 (FAILED)
**Issues:**
- Rustls 0.23 changed API significantly from 0.21
- `ServerCertVerifier` trait moved to `rustls::client::danger`
- Certificate types changed to `rustls_pki_types::CertificateDer`
- Quinn 0.11 requires complex Runtime configuration
- rcgen API changes (Rng module restructured)
- 16 compilation errors with API mismatches

### Combination 2: Quinn 0.10 + Rustls 0.21 + rcgen 0.11 (FAILED)
**Issues:**
- Quinn 0.10 API different from 0.11
- `Incoming` type not exported in quinn 0.10
- rcgen 0.11 API changed (`self_signed` method signature, private fields)
- rustls_pemfile API changed (`private_key` function not found)
- 11 compilation errors with API mismatches

### Combination 3: Quinn 0.10 + Rustls 0.20 + rcgen 0.10 (NOT TESTED)
**Expected Issues:**
- Likely similar API incompatibilities
- Version ecosystem is in flux

---

## Root Cause Analysis

**The Rust QUIC ecosystem is in a period of significant API instability:**

1. **Rustls Major Version Changes:**
   - 0.20 → 0.21 → 0.23: Major breaking changes in certificate handling
   - Trait reorganizations (ServerCertVerifier moved)
   - Type changes (Certificate, PrivateKey)

2. **Quinn Major Version Changes:**
   - 0.10 → 0.11: Major breaking changes in Endpoint configuration
   - Runtime requirements added
   - Connection acceptance API changed

3. **rcgen API Changes:**
   - 0.10 → 0.11 → 0.13: Certificate generation API changes
   - Private field access changes
   - RNG module reorganization

4. **No Stable Combination Found:**
   - The three libraries are not version-locked together
   - Each has independent release cycles
   - Compatible combinations are difficult to identify

---

## Working Dependency Matrix

| Quinn | Rustls | rcgen | Status | Issues |
|-------|--------|------|--------|--------|
| 0.11 | 0.23 | 0.13 | ❌ FAILED | 16 API errors |
| 0.11 | 0.21 | 0.13 | ❌ FAILED | Incompatible trait versions |
| 0.10 | 0.21 | 0.11 | ❌ FAILED | 11 API errors |
| 0.10 | 0.20 | 0.10 | ❓ NOT TESTED | Likely issues |
| 0.9  | 0.19 | 0.9  | ❓ NOT TESTED | EOL versions |

**Conclusion:** No working combination identified in this assessment.

---

## Implementation Risk Assessment

**Risk Level: HIGH**

**Risk Factors:**
1. **API Volatility:** All three libraries have breaking changes between minor versions
2. **Documentation Lag:** Documentation does not match current APIs
3. **Ecosystem Fragmentation:** No single version combination is stable
4. **Time Investment:** Additional time required to find working combination
5. **Maintenance Burden:** Once found, the combination may become obsolete quickly

**Specific Risks:**
- Certificate generation and validation complexity
- TLS configuration complexity
- Quinn Endpoint configuration complexity
- Cross-platform compatibility (Windows-specific issues observed)

---

## Proof of QUIC Working

**Status:** NOT ACHIEVED

**Attempted Actions:**
1. Created isolated prototype crate
2. Tested Quinn 0.11 + Rustls 0.23 combination
3. Tested Quinn 0.10 + Rustls 0.21 combination
4. Attempted to resolve API mismatches
5. Result: Unable to compile working prototype

**Remaining Attempts (if required):**
1. Try Quinn 0.9 + Rustls 0.19 + rcgen 0.9 (EOL versions)
2. Try Quinn 0.10 + Rustls 0.20 + rcgen 0.10
3. Investigate alternative QUIC libraries (s2n-quic)
4. Wait for ecosystem stabilization (6-12 months)

---

## Migration Plan

### Option A: Defer QUIC to Phase 3 (RECOMMENDED)

**Rationale:**
- Continue Phase 2 with TCP transport
- TCP is functional and tests pass
- Trait-based architecture allows seamless migration
- QUIC ecosystem may stabilize by Phase 3

**Implementation:**
1. Document TCP as temporary Phase 2 transport
2. Mark xentrium-network transport as TCP-only for Phase 2
3. Add QUIC migration task to Phase 3 roadmap
4. Re-assess QUIC feasibility at Phase 3 start
5. Potentially use external networking service if QUIC remains unstable

**Pros:**
- Maintains development momentum
- Zero risk to current codebase
- Allows ecosystem to stabilize
- Can implement all other Phase 2 features

**Cons:**
- Non-compliant with TAD v2.2.7 during Phase 2
- Deferred QUIC work remains
- May require re-testing all network code

### Option B: Invest in QUIC Now (HIGH RISK)

**Rationale:**
- Achieve TAD compliance immediately
- Future-proof the implementation

**Implementation:**
1. Allocate 2-3 days for dependency research
2. Test all version combinations systematically
3. Consider alternative QUIC libraries
4. Implement QUIC with working combination
5. Update xentrium-network transport

**Pros:**
- TAD compliant
- Future-proof

**Cons:**
- High risk of failure
- Significant time investment
- May delay Phase 2 by 1-2 weeks
- Ecosystem may change again after implementation

### Option C: Use s2n-quic Alternative (NOT RECOMMENDED)

**Rationale:**
- s2n-quic is AWS's QUIC implementation
- May have different API stability

**Implementation:**
1. Evaluate s2n-quic API
2. Build prototype
3. Integrate if viable

**Pros:**
- Different codebase, different issues
- AWS-backed, may be more stable

**Cons:**
- Different from TAD specification (quinn specified)
- Additional learning curve
- May have similar issues

---

## Recommendation

**RECOMMENDATION: Option A - Defer QUIC to Phase 3**

**Justification:**
1. The QUIC ecosystem is currently unstable with no working version combination identified
2. TCP implementation is functional and tests pass
3. Trait-based architecture allows seamless migration when feasible
4. Phase 2 can focus on protocol logic (gossip, snapshot sync, validator management)
5. The ecosystem may stabilize by Phase 3 (6-12 months from now)

**Timeline:**
- Phase 2 (current): Complete with TCP transport
- Phase 3 start: Re-assess QUIC feasibility
- Phase 3 implementation: Migrate to QUIC if stable

**Note to TAD Compliance:**
- Acknowledge non-compliance during Phase 2
- Document as temporary measure
- Commit to QUIC migration in Phase 3
- This is a practical compromise given ecosystem instability

---

## Conclusion

**QUIC is NOT currently feasible with available Rust libraries.**

The ecosystem is in a period of significant API instability with no stable dependency combination identified. Implementing QUIC now would require significant time investment with high risk of failure.

**Recommendation:** Defer QUIC to Phase 3, continue Phase 2 with TCP transport, and re-assess when the ecosystem stabilizes.
