# Verify security boundaries

The [security policy](security.md) defines intended authority and accepted risks.
The [threat reference](security-reference.md) explains threat IDs and earlier
findings. This page explains what evidence a boundary change needs; a test suite
passing is not a proof that arbitrary documents are harmless.

## Regression checklist

| Boundary | Evidence required |
| --- | --- |
| Entry classification | Direct local opens are Trusted; clipboard/web staging remains Untrusted |
| Navigation and restoration | Same-path tabs cannot borrow another origin's authority; restored sessions contain no grants |
| Local images | Untrusted reads require the identified resource; clipboard temporary directories grant no base authority |
| Network targets | Check resolved destinations and redirects; grants bind origin and address class |
| Grant lifetime | Another tab, replacement content or closed session cannot reuse a grant; Load all changes only admission count |
| Credentials and routing | Capture received requests; check userinfo, headers, redirects, unsafe ports and disabled proxy routing |
| Cache/offline | A cached body cannot bypass receiving authority; offline performs no request |
| Export | Preserve source mode and grants; headless missing authority fails without an interactive prompt |
| Async work | Late results cannot update replacement content or resurrect a closed tab |
| Processing limits | Exercise limits and their fallbacks in both modes, including decoded sizes and parser depth |

Inspect requests received by a local test service, not only decode success: a
GET may have an effect even if its response is not a valid image. Tests should
exercise entry points through their shared resolver and transport, rather than
mocking away the policy being checked.

Run the native suite using [testing setup](testing.md). Source owners include
`src/security`, `src/net`, `src/images`, application document/session handling
and export. Use [property tests](property-testing.md) for lifecycle sequences and
[fuzzing](../../fuzz/README.md) for malformed structured input.

## Remaining evidence

Do not promote a proposal into a guarantee. In particular:

- Per-resource budgets do not establish bounded aggregate process memory or
  execution time. Measure simultaneous parsing, shaping, decoding and retained tabs.
- Address-class and local redirect tests do not replace controlled DNS-rebinding
  evidence. Audit the actual connected destination, including mapped addresses.
- Example-based cancellation tests do not exhaust worker interleavings. A separate
  concurrency model needs explicit state and scheduling assumptions.
- Deterministic pinned-font fixtures do not establish cross-process determinism
  for arbitrary system fonts, drivers or dependencies.
- Finite input sizes and stylesheet numbers do not by themselves guarantee finite,
  reasonably bounded output geometry.
- Software-GPU tests cannot establish the safety of every graphics driver.
- Personal-reader permissions are not isolation for a multi-user conversion service.

The fuzz workspace owns the current target list, budgets and deferred surfaces;
[its findings](../../fuzz/FINDINGS.md) preserve reduced regressions. Check actual
harness reachability before claiming decoder, SVG, transport or platform coverage.
Dependency audit and sanitizer results need their tool versions, scope and run
revision recorded with the evidence. Absence of a recorded finding is not evidence
that an audit was performed.
