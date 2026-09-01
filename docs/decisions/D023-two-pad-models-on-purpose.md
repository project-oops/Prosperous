# D023 - Two pad models, on purpose


orbistoun and this project both hold a controller model, and the duplication is real. The part
worth keeping separate is smaller than it looks, and it is the part that matters.

**Its `PadState` is host-shaped floats**, and its own documentation says why: *"the guest-facing
layout is unmeasured and this type deliberately does not guess at it."* It describes what a
person is doing with their hands, which is knowable, and leaves the conversion to whatever a
title reads as a separate problem beginning with a measurement.

**This project's `Pad` is unsigned bytes centred on 128**, because that layout has since been
measured against real hardware. Matching it means the payload does no arithmetic, and
arithmetic in a payload is a thing that can be wrong somewhere nobody is looking.

Same question, different evidence, opposite answers - and both are right for their own project.
**Recorded so that neither gets "corrected" to match the other**, which is what would otherwise
happen the first time somebody noticed they disagree.

What genuinely is shared is the button *positions* and what is measured about them. A common
home for those is worth having; the encoding is not part of it, and neither is the keyboard
mapping - which is about a person and a keyboard rather than about a console.

**Amended.** This paragraph once listed tap-versus-hold among what is shared, which contradicts
the last paragraph below, where it is refused for a reason that still holds. The last paragraph
is the right one.

**One premise has since expired**, and the conclusion survives it. The quoted *"unmeasured"* was
true when this was written; the button bits have since been measured and are credited in
`ACKNOWLEDGEMENTS.md`. That does not merge the two models - it means there is a third thing,
neither of them, which is the measured facts themselves. `docs/PAD.md` proposes where those
should live.

Three of orbistoun's decisions were taken here and are credited in `ACKNOWLEDGEMENTS.md`. One
was refused for a reason worth stating: **its tap-versus-hold does not belong here.** It exists
because an emulator has to decide what a long press means. This project forwards absolute state
sixty times a second and the target decides - so implementing it here would duplicate the
target's own logic in the one place that cannot see the result.

