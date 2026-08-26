use crate::{VdfProof, VdfResult};
use num_bigint::BigUint;
use std::time::Duration;

pub trait VdfBackend: Send + Sync {
    fn prove(&self, input: &[u8], iterations: u64) -> VdfResult<VdfProof>;
    
    fn verify(&self, proof: &VdfProof) -> VdfResult<bool>;
    
    fn estimated_time(&self, iterations: u64) -> Duration;
    
    fn name(&self) -> &'static str;
}

#[derive(Debug, Clone)]
pub struct VdfParams {
    pub modulus: BigUint,
    pub security_parameter: u32,
}

impl Default for VdfParams {
    fn default() -> Self {
        // Using a 2048-bit RSA modulus for testing
        // In production, this should come from a trusted ceremony
        let modulus_str = "25195908475657893494027183240048398571429282126204032027777137836043662020707595556264018525880784406918290641249515082189298559149176184502808489120072844992687392807287776735971418347270261896375014971824691165077613379859095700097330459748808428401797429100642458691817195118746121515172654632282216869987549182422433637259085141865462043576798423387184774447920739934236584823824281198163815010674810451660377306056201619676256133844143603833904414952634432190114657544454178424020924616515723350778707749817125772467962926386356373289912154831438167899885040445364023527381951378636564391212010397122822120720357";
        let modulus = BigUint::parse_bytes(modulus_str.as_bytes(), 10)
            .expect("Failed to parse modulus");
        
        Self {
            modulus,
            security_parameter: 128,
        }
    }
}
