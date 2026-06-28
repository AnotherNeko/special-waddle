## Glossary

| Term | Definition |
|---|---|
| **Hotspot** | A player, machine, or reaction site that elevates the local cadence of surrounding cells. Analogous to a heat source raising reaction rates in thermodynamics. Leaves "nucleation site" reserved for actual phase-change or chemical reactions. |
| **Cadence** | The local tick rate of a region, expressed as a divisor of the global step clock. High cadence = steps frequently (small divisor). Low cadence = steps rarely (large divisor). Two regions (or the same region before/after the player arrived) can have different cadences while sharing the same physical time constant — the net equilibrium is the same, just approached at different wall-clock speeds. |
| **Cadence zone** | A contiguous region of cells sharing the same cadence. Boundaries between cadence zones are tempo seams. |
| **Cadence refinement** | Raising the cadence of a zone (shrinking the divisor) to increase temporal fidelity in that area. "Refinement" here always means cadence refinement, not spatial or mesh refinement — spatial refinement would be confused with chemical refinery operations. |
| **Refinement anchor** | A placed object (machine, beacon, or player presence) that pins a cadence zone to high cadence for as long as it remains. Analogous to a dimensional anchor or chunk loader: it holds the local simulation rate in place against the natural drift toward low cadence. |
| **Tempo seam** | The boundary plane between two cadence zones. Implemented as a `Buffered` contract: the high-cadence side accumulates flow across its ticks, the low-cadence side drains the buffer on its tick. Mass is conserved across the seam. |
| **Nucleation site** | A location where a phase-change or chemical reaction can initiate (e.g., bubble formation, crystallization). Distinct from a hotspot: a nucleation site is a physics phenomenon, not a scheduling concept. |
