CONTENTS: lines 1-8 is TLDR, lines 9-22 are key findings, and lines 23-81 are the gritty details you might need to thoroughly implement a conserved field without deriving the conservation laws from principles.
# optozorax's Portal-Gravity FEM Formulation and Its Application to Thermal Diffusion on Voxels

## TL;DR
- **optozorax (Ilya Sheprut) concludes that to conserve energy, the gravitational/Newtonian potential field must "flow through" portals — the two portal mouths are *identified* so the potential and its flux are continuous (smooth) across them, making the field conservative and killing the infinite-acceleration paradox.** The governing equation is the source-free Laplace problem ∇²φ = 0, solved by the Galerkin FEM.
- **The portal is a *topological modification of the mesh*, not an external Dirichlet/Neumann wall: the degrees of freedom on the two mouths are coupled symmetrically, which in weak-form FEM means the boundary flux term out of face A is matched by the flux into face B (zero net jump). Spatial separation of the mouths is irrelevant ("surface irrelevance") — no distance correction is applied.** This maps directly onto a sparse, symmetric off-diagonal coupling in your voxel stiffness/Laplacian matrix.
- **For your immediate blocker: a symmetric portal pair (and a cross-server "remote" pair) must use symmetric coupling K[A,B] = K[B,A] to conserve the field; a "void" (one mouth → sink) is NOT a degenerate symmetric portal pair — it is a separate primitive that is inherently *directed* and is best modeled as a Dirichlet/absorbing sink. Symmetric coupling conserves heat (no sink); a one-way coupling or per-cell loss term breaks stiffness-matrix symmetry, which is exactly the discrete signature a sink should have.**

## Key Findings

1. **Thesis (confirmed verbatim from the author).** In the video *"Portals must bend gravity, actually"* (optozorax channel, published December 10, 2025; a "40-minute video, 5 months of work, 230 hours of programming" per his Mathstodon announcement), the central claim is stated verbatim in the description: *"Portals don't conserve energy? Actually, no. We just need to allow gravity to flow through portals. And once we do that, portals start to conserve energy and make sense physically."* His Mathstodon post (mathstodon.xyz/@optozorax/115696771586540697, Dec 10, 2025) frames it as: *"Do portals violate conservation of energy? No! You just need to teleport gravity through them. How do you do that? And what does the Finite Element Method have to do with it?"* (optozorax is Ilya Sheprut, of Novosibirsk State Technical University.)

2. **The physics model.** The gravitational potential φ obeys **Laplace's equation ∇²φ = 0** everywhere except at sources/boundaries. The community Portal Theory Wiki ("Portals and Gravity"), which documents exactly this model, gives the solver principle verbatim as *"∂t/∂ϕ = ∇²ϕ because we want ∇²ϕ = 0 everywhere (except the very boundary)"* — i.e., a heat-equation-style relaxation that converges to the harmonic (steady-state) solution.

3. **Why energy conservation requires flux teleportation.** The paradox: a particle falling through a vertically-stacked portal pair accelerates forever, gaining unbounded kinetic energy. The fix is that gravity must be transmitted through the portal so the field is *smooth (C¹-continuous) across the portal interface*. The wiki states the result verbatim: *"It is indeed a conservative field and conservation laws work. So, the infinite ever accelerating falling paradox is solved: acceleration along either direction is cancelled out on closed paths."* The conservation condition is that the line integral of the field (∇φ) around any closed loop through the portal is zero — equivalently, flux leaving one mouth equals flux entering the other.

4. **FEM method.** optozorax uses the standard **Galerkin weak formulation** of the Laplacian (he explicitly cites DrSimulate's "I Finally Understood The Weak Formulation" as background, and links his own university FEM coursework repo, `github.com/optozorax/labs_emf`, "Equations of Mathematical Physics," C++/TeX). The chapters — confirmed verbatim from the video's chapter list as *"29:40 Finite Element Method,"* *"35:34 How to add portals to the FEM,"* and *"39:18 Conclusion"* — build the discrete Laplacian/stiffness matrix and then modify it to insert portals.

5. **How portals enter the FEM.** Portals are added by **coupling the mesh across the two mouths** — the two portal faces are treated as one shared interface ("surface irrelevance": the field behaves as if the two faces are directly adjacent, regardless of their real separation). This is a topological identification of degrees of freedom, not an imposed Dirichlet/Neumann boundary value. In weak-form terms, the integration-by-parts boundary term ∫ v (∇φ·n) dS on mouth A is set equal and opposite to the corresponding term on mouth B, so the two flux contributions cancel and the global stiffness matrix stays symmetric and conservative.

6. **The "void" / single-mouth case.** No accessible source (video, Mathstodon, the Portal Theory Wiki, his blog) explicitly addresses a one-mouth "portal to nowhere." optozorax's framework is built on *pairs* (or n-tuples) of mouths with the field summed over all paths. This strongly implies a void is **not** a degenerate symmetric portal pair within his conservative model — a lone mouth that absorbs flux is a genuine sink and breaks the conservation that the paired model is designed to enforce.

## Details

### The governing equations and the discrete operator
For the static potential, the continuous problem is Laplace/Poisson:

  ∇²φ = ρ (with ρ = 0 in free space; sources where mass exists).

The weak form: find φ such that ∫ ∇φ·∇v dΩ = −∫ ρ v dΩ + (boundary flux terms) for all test functions v. Discretizing with nodal basis functions Nᵢ gives the stiffness matrix

  Kᵢⱼ = ∫ ∇Nᵢ·∇Nⱼ dΩ,

and the linear system K φ = f. On a **uniform voxel grid**, this reduces to the familiar 7-point (3-D) discrete Laplacian: each interior cell i has diagonal Kᵢᵢ = Σ (conductances to neighbors) and off-diagonal Kᵢⱼ = −(face conductance) for each of its 6 neighbors. For unit spacing and unit conductivity, Kᵢᵢ = 6 and Kᵢⱼ = −1. (optozorax's iterative solver "∂t/∂φ = ∇²φ" is precisely the explicit-Euler relaxation of this operator toward steady state, which is structurally identical to your thermal diffusion update.)

### Inserting a portal pair into the matrix (the core construction)
Let face A be a voxel face of cell a, and face B be the paired face of cell b, with a and b arbitrarily far apart in space. To make a portal:

1. **Sever the natural neighbor link across each portal face's physical (now-walled) side** if the portal replaces a solid wall; or keep the local neighbors as normal if the portal is an internal membrane. (The portal face stops being adjacent to whatever was physically behind it and instead becomes adjacent to the other mouth.)
2. **Add a symmetric coupling between a and b:** K[a,b] += −g, K[b,a] += −g, and add the matching diagonal terms K[a,a] += g, K[b,b] += g, where g is the face conductance (for a normal internal face on a unit grid, g = 1; the portal face then behaves exactly like an ordinary internal face).

This is the matrix realization of "gravity flows through the portal." Crucially the coupling is **symmetric** (K[a,b] = K[b,a]); symmetry of K is exactly what guarantees the operator is self-adjoint/conservative — the discrete analogue of "the field is conservative and closed-loop work is zero." This is also why optozorax stresses the field must remain smooth across the portal: in the discrete picture, smoothness = the same off-diagonal coupling appears on both sides.

### The "distance mismatch" between mouths
optozorax's answer is **surface irrelevance**: the two mouths are glued as if there is *no* distance between them. The gradient/flux across the portal is computed using the *local* cell-to-face geometry on each side (half a voxel on each side, just like any internal face), **not** the Euclidean distance between the two mouths in ambient space. There is therefore **no distance-based correction term** — the coupling conductance g is the ordinary face conductance. The "mismatch" in the paradox (the two points being at different real distances from Earth) is precisely what gets *removed* by this gluing: through the portal the two points are forced to feel the same potential coupling, which cancels the runaway acceleration. (His related "space bending" visualization makes the same point geometrically: through a portal "the space just became adjacent, as if there's no distance between its parts," and he concludes "portals are wormholes.")

### Boundary-condition classification
In FEM taxonomy, the portal is **neither a Dirichlet nor a Neumann nor a Robin boundary condition** in the classical "value/derivative prescribed on an exterior boundary" sense. It is an **internal interface / topological identification** with two matching conditions:
- **Jump in value = 0:** ⟦φ⟧ = φ_A − φ_B = 0 across the glued interface (continuity of potential).
- **Jump in flux = 0:** the outward normal flux on A plus the outward normal flux on B sum to zero (no field is created or destroyed at the portal) — this is the conservation condition.

Operationally on a voxel grid you implement this not by adding rows for a boundary but by **adding off-diagonal entries** that connect the two interior cells — exactly the sparse per-pair connectivity-graph override your architecture already targets.

### The void: symmetric vs. directed — the decision
A **symmetric portal pair conserves the field**: whatever flux leaves mouth A enters mouth B, so the global "amount" of the diffusing quantity (heat) is preserved and the stiffness matrix stays symmetric. A **void is the opposite of conservative — it is a sink** — so it cannot be a symmetric coupling. The two clean ways to model a void cell are:

- **(Recommended) Dirichlet/absorbing sink:** pin the void cell's value to a fixed reference (e.g., T = 0, or ambient) and let its neighbors diffuse into it. Heat flows in and disappears. This is a one-row override (fixed value), simple and unconditionally stable, and it is *directed* in effect (net flux is always inward to the sink). This makes the void a **separate primitive**, not a portal pair.
- **Directed (asymmetric) flux removal:** add a one-way coupling K[neighbor, void] = −g without the reciprocal K[void, neighbor], or equivalently a per-cell loss term Kᵢᵢ += g_loss that removes flux proportional to the local value (a Robin-type "leak to infinity"). This makes the stiffness matrix **asymmetric / non-symmetric-PD**, which is the discrete signature of a non-conservative sink — physically correct for a void, but you lose the symmetric-solver guarantees and must use a solver that tolerates non-symmetry (e.g., BiCGSTAB/GMRES) or treat the loss as an explicit per-step decay.

**Conclusion for your blocker:** A void is *directed*, not symmetric, and is best treated as a Dirichlet/absorbing sink (a separate primitive) rather than as "half of a portal pair." Reserve the symmetric K[A,B]=K[B,A] coupling exclusively for true two-mouth portals (local or remote/cross-server). A cross-server "remote" voxel pair is mathematically identical to a local portal pair — symmetric coupling, no distance correction — the only extra requirement is that *both* servers insert the *same* off-diagonal conductance g and exchange the two boundary values each step so the coupling stays consistent; any asymmetry introduced by lag or one-sided updates will manifest as spurious energy gain/loss, exactly like the original portal paradox.

## Recommendations

1. **Model the conservative primitives (local portal pair, remote/cross-server pair) as symmetric off-diagonal couplings.** For each pair (a,b): K[a,b] = K[b,a] = −g and K[a,a],K[b,b] += g, with g = the ordinary internal-face conductance (no distance scaling). Verify conservation by checking that the column sums of the coupling block are zero (each added off-diagonal balanced by a diagonal term).

2. **Model the void as a Dirichlet sink first.** Pin void cells to a fixed reference value and keep K symmetric for everything else. This is the lowest-risk path and preserves your symmetric/CG solver. Only escalate to an asymmetric one-way coupling or per-cell decay term if you need the void to have finite, tunable absorption rather than infinite-conductance pinning.

3. **Keep the portal coupling in the *stiffness/Laplacian* term, not as a source.** This guarantees the discrete operator remains a graph Laplacian (symmetric, weakly diagonally dominant) for portals, giving you stability and conservation for free. Sources/sinks (voids) live in the diagonal or the RHS.

4. **For cross-server pairs, exchange boundary values (ghost cells), not fluxes, and apply identical g on both sides.** Treat the remote cell as a ghost cell whose value is synced each tick; compute the symmetric coupling locally on each server. Validate with a closed-loop test (heat injected, transported through a remote portal, and back) and confirm total-energy drift ≈ 0.

5. **Benchmarks that change the recommendation:**
   - If a closed-loop test through a portal pair shows monotonic energy gain or loss, your coupling is asymmetric (likely a sync/lag bug on the remote pair, or a missing diagonal balance) — fix symmetry before anything else.
   - If voids modeled as Dirichlet sinks cause checkerboard/instability at large time steps, switch to the per-cell decay (Robin) form, which is gentler.
   - If you later need *gravity-like* (not heat-like) behavior — e.g., levitation/equilibrium points (the wiki notes stable zero-field points exist in optozorax's model) — note that for a pure diffusion field these correspond to steady-state plateaus, not force balance, so the analogy ends at the operator level.

## Caveats
- **Primary-source limitation:** The exact equations, stiffness-matrix wording, and the precise phrasing of any "void" discussion from the video's chapters at 29:40, 35:34, and 39:18 could **not** be extracted verbatim — YouTube served a bot-protection page and every transcript service rendered content client-side. The FEM-portal mechanics above are reconstructed from (a) the confirmed video description and chapter list, (b) optozorax's Mathstodon posts, (c) the Portal Theory Wiki "Portals and Gravity" page that documents this exact model, and (d) standard interface-FEM theory. Statements directly attributable to optozorax are quoted; the matrix-level construction and the void analysis are my engineering synthesis consistent with his stated conservation principle, not verbatim claims from him. To obtain timestamped verbatim content, open the video and click "Show transcript" in a real browser, or run youtubetotranscript.com / tactiq.io client-side.
- **The void question is unsourced in optozorax's work.** He does not (in any accessible material) treat a one-mouth void; the symmetric-vs-directed conclusion is derived from the conservation logic of his model, not stated by him. Treat it as a well-grounded inference, not a citation.
- **Gravity vs. heat disanalogy.** optozorax solves a *static* Laplace problem (∇²φ = 0) for a potential whose gradient is a force; you are solving a *time-dependent* diffusion/thermal problem (∂u/∂t = α∇²u). The spatial operator and the portal coupling are identical, but the conserved quantities differ (he conserves energy along trajectories; you conserve total heat). The portal-coupling construction transfers cleanly; the interpretation of "levitation/equilibrium points" does not.
- **Follow-up source.** A successor video, *"Portals can simulate a Klein bottle, so I calculated gravity on it"* (youtube.com/watch?v=Q36ULjrK6Po), described in its own intro as "Continuing to explore how portals affect gravity," continues the same FEM-gravity method and may restate the interface coupling more explicitly if you can access its captions.
