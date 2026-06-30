## Glossary

| Term | Definition |
|---|---|
| **Hotspot** | A player, machine, or reaction site that elevates the local cadence of surrounding cells. Analogous to a heat source raising reaction rates in thermodynamics. Leaves "nucleation site" reserved for actual phase-change or chemical reactions. |
| **Cadence** | The local tick rate of a region, expressed as a divisor of the global step clock. High cadence = steps frequently (small divisor). Low cadence = steps rarely (large divisor). Two regions (or the same region before/after the player arrived) can have different cadences while sharing the same physical time constant — the net equilibrium is the same, just approached at different wall-clock sample rates. Since the sample rate affects sim quality, only shorter time constant processes are distorted by slow cadence, but longer time constant processes are not affected - the time constant (as percieved on wall time) is preserved as the cadence varies. |
| **Cadence zone** | A contiguous region of cells sharing the same cadence. Boundaries between cadence zones are tempo seams. |
| **Cadence refinement** | Raising the cadence of a zone (shrinking the divisor) to increase temporal fidelity in that area. "Refinement" here always means cadence refinement, not spatial or mesh refinement — spatial refinement would be confused with chemical refinery operations. |
| **Refinement anchor** | A placed object (machine, beacon, or player presence) that pins a cadence zone to high cadence for as long as it remains. Analogous to a dimensional anchor or chunk loader: it holds the local simulation rate in place against the natural drift toward low cadence. |
| **Tempo seam** | The boundary plane between two cadence zones. Implemented as a `Buffered` contract: the high-cadence side accumulates flow across its ticks, the low-cadence side drains the buffer on its tick. Mass is conserved across the seam. |
| **Nucleation site** | A location where a phase-change or chemical reaction can initiate (e.g., bubble formation, crystallization). Distinct from a hotspot: a nucleation site is a physics phenomenon, not a scheduling concept. |
| **node** | A fundamental cubic unit of a world and appears to a player as roughly 1x1x1 meters in size. |
| **mapblock** (aka **tile** in the rust implementation) | 16x16x16 nodes and the fundamental region of a world that is stored in the world database, sent to clients, and handled by many parts of the engine. Available as constant `core.MAP_BLOCKSIZE` (=16). |
| **mapblock** | Preferred terminology to 'block' to avoid confusion with 'node'; however, 'block' often appears in the API. |
| **mapchunk** | Usually 5x5x5 mapblocks (80x80x80 nodes), the volume of world generated in one map generator operation, with size optimized for efficient map generation.
