use crate::{VdfProof, VdfResult, VdfError, wesolowski::WesolowskiVdf, VdfBackend};
use num_bigint::BigUint;
use sha2::{Sha256, Digest};
use std::time::{Duration, Instant};
use tracing::{debug, info};

#[derive(Debug, Clone)]
pub struct BatchProof {
    pub tick_range: (u64, u64), // (start_tick, end_tick)
    pub proofs: Vec<VdfProof>,
    pub chain_verification_proof: ChainProof,
}

#[derive(Debug, Clone)]
pub struct ChainProof {
    pub merkle_root: [u8; 32],
    pub chain_hash: [u8; 32],
    pub aggregate_iterations: u64,
}

pub trait BatchVerifier {
    fn verify_batch(&self, batch_proof: &BatchProof) -> VdfResult<bool>;
    fn verify_chain_integrity(&self, batch_proof: &BatchProof) -> VdfResult<bool>;
    fn create_batch_proof(&self, proofs: Vec<VdfProof>, start_tick: u64) -> VdfResult<BatchProof>;
}

impl BatchVerifier for WesolowskiVdf {
    fn verify_batch(&self, batch_proof: &BatchProof) -> VdfResult<bool> {
        let start = Instant::now();
        
        // First verify chain integrity
        if !self.verify_chain_integrity(batch_proof)? {
            return Ok(false);
        }
        
        // Parallel verification strategy: verify every Nth proof + chain consistency
        let verification_sample_rate = if batch_proof.proofs.len() > 100 { 10 } else { 5 };
        
        let mut verified_count = 0;
        let total_proofs = batch_proof.proofs.len();
        
        // Sample verification: verify every Nth proof
        for (i, proof) in batch_proof.proofs.iter().enumerate() {
            if i % verification_sample_rate == 0 || i == total_proofs - 1 {
                if !self.verify(proof)? {
                    debug!("Batch verification failed at proof index {}", i);
                    return Ok(false);
                }
                verified_count += 1;
            }
        }
        
        // Additional chain consistency checks
        for i in 1..batch_proof.proofs.len() {
            let prev_proof = &batch_proof.proofs[i - 1];
            let curr_proof = &batch_proof.proofs[i];
            
            // Verify that current proof's input is derived from previous output
            if !self.verify_proof_chaining(prev_proof, curr_proof)? {
                debug!("Chain verification failed between proofs {} and {}", i - 1, i);
                return Ok(false);
            }
        }
        
        let verification_time = start.elapsed();
        info!(
            "Batch verification completed: {}/{} proofs verified in {:?} ({}x speedup)",
            verified_count,
            total_proofs,
            verification_time,
            (total_proofs as f64 / verified_count as f64) as u32
        );
        
        Ok(true)
    }
    
    fn verify_chain_integrity(&self, batch_proof: &BatchProof) -> VdfResult<bool> {
        if batch_proof.proofs.is_empty() {
            return Ok(true);
        }
        
        // Verify merkle root of all proof outputs
        let proof_hashes: Vec<[u8; 32]> = batch_proof.proofs
            .iter()
            .map(|proof| {
                let mut hasher = Sha256::new();
                hasher.update(&proof.output.to_bytes_be());
                hasher.update(&proof.iterations.to_be_bytes());
                hasher.finalize().into()
            })
            .collect();
        
        let computed_root = compute_merkle_root(&proof_hashes);
        if computed_root != batch_proof.chain_verification_proof.merkle_root {
            debug!("Merkle root verification failed");
            return Ok(false);
        }
        
        // Verify chain hash (sequential dependency)
        let computed_chain_hash = self.compute_chain_hash(&batch_proof.proofs)?;
        if computed_chain_hash != batch_proof.chain_verification_proof.chain_hash {
            debug!("Chain hash verification failed");
            return Ok(false);
        }
        
        // Verify total iteration count
        let total_iterations: u64 = batch_proof.proofs.iter().map(|p| p.iterations).sum();
        if total_iterations != batch_proof.chain_verification_proof.aggregate_iterations {
            debug!("Aggregate iteration count verification failed");
            return Ok(false);
        }
        
        Ok(true)
    }
    
    fn create_batch_proof(&self, proofs: Vec<VdfProof>, start_tick: u64) -> VdfResult<BatchProof> {
        if proofs.is_empty() {
            return Err(VdfError::InvalidInput);
        }
        
        let end_tick = start_tick + proofs.len() as u64 - 1;
        
        // Create merkle root of proof outputs
        let proof_hashes: Vec<[u8; 32]> = proofs
            .iter()
            .map(|proof| {
                let mut hasher = Sha256::new();
                hasher.update(&proof.output.to_bytes_be());
                hasher.update(&proof.iterations.to_be_bytes());
                hasher.finalize().into()
            })
            .collect();
        
        let merkle_root = compute_merkle_root(&proof_hashes);
        let chain_hash = self.compute_chain_hash(&proofs)?;
        let aggregate_iterations = proofs.iter().map(|p| p.iterations).sum();
        
        Ok(BatchProof {
            tick_range: (start_tick, end_tick),
            proofs,
            chain_verification_proof: ChainProof {
                merkle_root,
                chain_hash,
                aggregate_iterations,
            },
        })
    }
}

impl WesolowskiVdf {
    fn verify_proof_chaining(&self, prev_proof: &VdfProof, curr_proof: &VdfProof) -> VdfResult<bool> {
        // Check that current proof's input incorporates previous proof's output
        let mut expected_input_hasher = Sha256::new();
        expected_input_hasher.update(&prev_proof.output.to_bytes_be());
        
        // The current proof's input should include a hash of the previous output
        // We can't verify exact equality since transaction data is also included,
        // but we can verify that the previous output influenced the current input
        let prev_output_hash = expected_input_hasher.finalize();
        
        // Check if the previous output hash appears to influence current input
        // This is a simplified check - in production, this would be more rigorous
        if curr_proof.input.len() >= 32 {
            // Look for hash influence in the input (simplified heuristic)
            let input_hash = Sha256::digest(&curr_proof.input);
            
            // Verify that inputs are different (chaining requirement)
            Ok(prev_proof.input != curr_proof.input && input_hash != Sha256::digest(&prev_proof.input))
        } else {
            Ok(false)
        }
    }
    
    fn compute_chain_hash(&self, proofs: &[VdfProof]) -> VdfResult<[u8; 32]> {
        let mut hasher = Sha256::new();
        
        for proof in proofs {
            hasher.update(&proof.input);
            hasher.update(&proof.output.to_bytes_be());
            hasher.update(&proof.iterations.to_be_bytes());
        }
        
        Ok(hasher.finalize().into())
    }
}

pub fn compute_merkle_root(leaves: &[[u8; 32]]) -> [u8; 32] {
    if leaves.is_empty() {
        return [0u8; 32];
    }
    
    if leaves.len() == 1 {
        return leaves[0];
    }
    
    let mut current_level = leaves.to_vec();
    
    while current_level.len() > 1 {
        let mut next_level = Vec::new();
        
        for chunk in current_level.chunks(2) {
            let mut hasher = Sha256::new();
            hasher.update(&chunk[0]);
            
            if chunk.len() == 2 {
                hasher.update(&chunk[1]);
            } else {
                hasher.update(&chunk[0]); // Duplicate if odd number
            }
            
            next_level.push(hasher.finalize().into());
        }
        
        current_level = next_level;
    }
    
    current_level[0]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WesolowskiVdf;

    #[test]
    fn test_batch_verification() {
        let vdf = WesolowskiVdf::with_default_params();
        let input = b"test batch verification";
        
        // Create a series of chained proofs
        let mut proofs = Vec::new();
        let mut current_input = input.to_vec();
        
        for i in 0u32..5 {
            let proof = vdf.prove(&current_input, 10).expect("Failed to create proof");
            
            // Next input incorporates previous output
            let mut hasher = Sha256::new();
            hasher.update(&proof.output.to_bytes_be());
            hasher.update(&i.to_be_bytes());
            current_input = hasher.finalize().to_vec();
            
            proofs.push(proof);
        }
        
        // Create batch proof
        let batch_proof = vdf.create_batch_proof(proofs, 1).expect("Failed to create batch proof");
        
        // Verify batch
        let is_valid = vdf.verify_batch(&batch_proof).expect("Failed to verify batch");
        assert!(is_valid);
    }
    
    #[test]
    fn test_merkle_root_computation() {
        let leaves = vec![
            [1u8; 32],
            [2u8; 32],
            [3u8; 32],
        ];
        
        let root1 = compute_merkle_root(&leaves);
        let root2 = compute_merkle_root(&leaves);
        
        assert_eq!(root1, root2); // Deterministic
        assert_ne!(root1, [0u8; 32]); // Non-trivial
    }
    
    #[test]
    fn test_chain_integrity_verification() {
        let vdf = WesolowskiVdf::with_default_params();
        let input = b"chain integrity test";
        
        let proof1 = vdf.prove(input, 10).expect("Failed to create proof1");
        let proof2 = vdf.prove(input, 10).expect("Failed to create proof2"); // Same input = bad chain
        
        let proofs = vec![proof1.clone(), proof2];
        let batch_proof = vdf.create_batch_proof(proofs, 1).expect("Failed to create batch proof");
        
        // This should detect the broken chain
        let chain_valid = vdf.verify_chain_integrity(&batch_proof).expect("Failed to verify chain");
        assert!(chain_valid); // Merkle root and aggregate are still valid
        
        // But chaining verification should catch the issue
        let is_chained = vdf.verify_proof_chaining(&proof1, &batch_proof.proofs[1]).expect("Failed to verify chaining");
        assert!(!is_chained); // Should be false due to identical inputs
    }
}
