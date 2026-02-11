To keep the physics consistent across your non-uniform grid, we’ll treat every contract (including mirror types) as a unique pipe with its own "flow resistance" derived from geometry.
In this model, a Mirror Contract isn't an edge case to skip—it’s just a participant where $V_{neighbor} = V_{self}$, naturally resulting in zero flux without needing a conditional if branch in your hot loop.
1. The Global Variables
Before the formulas, here is the Quantity Calculus context for the system:
Variable
	Name
	Dimension
	Unit
	Source / Value
	$V$
	Potential (State)
	$[\Theta]$
	e.g., $K$
	Cell Attribute ($u32$)
	$\Delta V$
	Gradient
	$[\Theta]$
	$K$
	$(V_{self} - V_{neighbor})$
	$C_{mat}$
	Conductivity
	$[1]$
	Fraction
	Material Property ($u16 / 2^{16}$)
	$N_{base}$
	Stability Floor
	$[1]$
	Count
	Constant ($6 + 1 = 7$)
	$S_{face}$
	Subdivision
	$[1]$
	Count
	Contracts per Face (e.g., $1, 16, 1024$)
	$L$
	Side Length
	$[L]$
	Meters $[m]$
	Cell Geometry
	$L_{min}$
	Unit Length
	$[L]$
	Meters $[m]$
	Smallest possible cell side
	________________
2. Base Stability Formula (Non-Spatial)
This formula ensures that regardless of how many subnodes a neighbor has, or how many mirror contracts exist at the boundary, the cell remains thermodynamically stable.


$$\Delta \Phi = \frac{\Delta V \cdot C_{mat}}{N_{base} \cdot S_{face}}$$
* Logic: Every face has a budget of $1/7^{th}$ of the potential. If a face is subdivided into 1024 tiny contracts, each contract gets $1/1024^{th}$ of that face's budget.
* Mirror Case: If a face is a mirror, $\Delta V = 0$, so $\Delta \Phi = 0$. The "budget" for that face is effectively reserved but never spent.
________________
3. General Spatially-Scaled Formula
This introduces $dx$ (center-to-center distance). Smaller cells have centers that are closer together, creating a steeper gradient and faster conduction, which is essential for Level of Detail (LOD) consistency.


$$\Delta \Phi = \frac{\Delta V \cdot C_{mat} \cdot L_{min}}{N_{base} \cdot S_{face} \cdot \left( \frac{L_{self} + L_{neighbor}}{2} \right)}$$
* Scaling: As $L$ decreases (finer detail), the denominator decreases, and $\Delta \Phi$ (the flux) increases.
* Unit Check: The $L$ dimensions cancel out ($L_{min} / L_{avg}$), keeping the result in the dimension of the Potential $[\Theta]$.
________________
4. Octree-Optimized Formula
In an Octree, side lengths are always powers of 2: $L = 2^k \cdot L_{min}$. We can simplify the distance calculation and the subdivision factor using bit-shifts.
Let $k_{self}$ and $k_{neigh}$ be the octree levels (where 0 is the smallest cell).


$$\Delta \Phi = \frac{\Delta V \cdot C_{mat}}{7 \cdot S_{face} \cdot (2^{k_{self}-1} + 2^{k_{neigh}-1})}$$
Why this is faster for your code:
1. $S_{face}$ Calculation: If the neighbor is at a higher level than the current cell, $S_{face}$ is always 1. If the current cell is larger, $S_{face} = 4^{(k_{self} - k_{neigh})}$.
2. The Denominator: $(2^{k_{self}-1} + 2^{k_{neigh}-1})$ can be pre-calculated or stored as a small look-up table for level differences, since octrees usually only allow a level difference of 1 between adjacent cells.
Implementation Note for Fixed-Point
Since you are likely using integer math, remember that $C_{mat}$ is scaled by $2^{16}$. Your final bit-shift should happen after all multiplications to maintain precision:


$$\text{delta\_u32} = \frac{(\Delta V \cdot C_{u16} \cdot 2)}{7 \cdot S_{face} \cdot (L_{self} + L_{neighbor}) \cdot 2^{16}}$$
Would you like me to generate the bit-shift logic for the $S_{face}$ calculation specifically for your Octree traversal?



So I was tackling an issue with the conservation of mass test failing in some test variants in incremental.rs (similar tests in field.rs), it seems like rate of diffusion formula was seriously underdeveloped and there's a max(0) which creates mass in some conditions which should never occur. I've developed some more robust formulas for the rate of diffusion calculation, ensuring that the mass is conserved and that the diffusion process is stable without edge cases.
