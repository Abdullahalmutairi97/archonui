Display :117. TASK: MOTION — match every transition in the new UI to the prototype. Sources: docs/design/*.md (grep transition / ms / ease / cubic-bezier / animation / keyframes) and the prototype HTML (grep `transition:` / `@keyframes` / `animation:`). Infra: src/ui/anim.rs (Animator, easing, Transition consts, tick, reduced_motion()), GPUI `with_animation`.
1. Table → .qa/integration/motion-report.md: each spec'd transition (what, property, duration, easing) → our code location → matches / wrong / missing.
2. Implement only motion the prototype actually has, exact timings/easing; never change static look. Respect reduced_motion() (instant).
3. GPUI limits: one hover style per element (twice crashes), 2-stop gradients. Idle CPU must stay ~0% (`top -b -n 3 -p PID` before/after).
Small surgical edits; re-read before editing; never rewrite whole files; don't touch src/ui/approval.rs. Verify timings with frame captures for sidebar, panel, modal, popover, toast. Report (under 25 lines).
