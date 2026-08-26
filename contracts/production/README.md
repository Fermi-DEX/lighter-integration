# Production settlement join (reference)

`ContinuumLighterBinding.sol` is an interface and invariant scaffold, not a
drop-in replacement for Lighter's settlement contract. It shows the atomic
state transition that the production contract must enforce after verifying:

1. a `SequenceTransitionProof` against the pinned Continuum verifier;
2. a Lighter execution proof against the pinned execution verifier; and
3. equality of both proofs' `C_bind` with the value committed through the
   versioned Lighter blob header.

The actual Lighter patch must connect these public-input hashes to its existing
batch commitment, KZG/blob checks, governance, verifier upgrades, priority
queue, and escape hatch. A protected batch must never settle through this
scaffold alone.
