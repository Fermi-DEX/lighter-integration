use crate::{VdfBackend, VdfProof, VdfResult, VdfError, hash_to_prime};
use crate::backend::VdfParams;
use num_bigint::BigUint;
use std::time::Duration;
use sha2::{Sha256, Digest};

/// FPGA-accelerated VDF backend for AWS F1 instances
#[derive(Debug, Clone)]
pub struct FpgaVdfBackend {
    params: VdfParams,
    device_id: u32,
    fpga_available: bool,
}

impl FpgaVdfBackend {
    pub fn new(params: VdfParams, device_id: u32) -> VdfResult<Self> {
        let fpga_available = Self::check_fpga_availability(device_id)?;
        
        Ok(Self {
            params,
            device_id,
            fpga_available,
        })
    }
    
    pub fn with_default_params() -> VdfResult<Self> {
        Self::new(VdfParams::default(), 0)
    }
    
    /// Check if FPGA device is available and properly configured
    fn check_fpga_availability(device_id: u32) -> VdfResult<bool> {
        // Check for FPGA device files
        if std::path::Path::new(&format!("/dev/xdma{}_user", device_id)).exists() {
            Ok(true)
        } else {
            // Fallback to CPU implementation if FPGA not available
            eprintln!("Warning: FPGA device {} not found, falling back to CPU", device_id);
            Ok(false)
        }
    }
    
    /// Hardware-accelerated sequential squaring on FPGA
    fn fpga_sequential_squaring(&self, base: &BigUint, iterations: u64) -> VdfResult<BigUint> {
        if !self.fpga_available {
            return self.cpu_fallback_sequential_squaring(base, iterations);
        }
        
        // Convert BigUint to FPGA-compatible format
        let base_bytes = self.bigint_to_fpga_format(base);
        let modulus_bytes = self.bigint_to_fpga_format(&self.params.modulus);
        
        // Prepare FPGA command structure
        let fpga_command = FpgaVdfCommand {
            operation: VdfOperation::SequentialSquaring,
            base: base_bytes,
            modulus: modulus_bytes,
            iterations,
        };
        
        // Execute on FPGA
        match self.execute_fpga_command(&fpga_command) {
            Ok(result_bytes) => {
                let result = self.fpga_format_to_bigint(&result_bytes);
                Ok(result)
            }
            Err(_) => {
                eprintln!("FPGA execution failed, falling back to CPU");
                self.cpu_fallback_sequential_squaring(base, iterations)
            }
        }
    }
    
    /// CPU fallback for when FPGA is unavailable
    fn cpu_fallback_sequential_squaring(&self, base: &BigUint, iterations: u64) -> VdfResult<BigUint> {
        let mut result = base.clone();
        let modulus = &self.params.modulus;
        
        for _ in 0..iterations {
            result = (&result * &result) % modulus;
        }
        
        Ok(result)
    }
    
    /// Convert BigUint to FPGA 2048-bit format (256 bytes)
    fn bigint_to_fpga_format(&self, value: &BigUint) -> Vec<u8> {
        let bytes = value.to_bytes_le(); // Little-endian for FPGA
        let mut fpga_bytes = vec![0u8; 256]; // 2048 bits = 256 bytes
        
        // Copy value bytes, padding with zeros if necessary
        let copy_len = std::cmp::min(bytes.len(), 256);
        fpga_bytes[..copy_len].copy_from_slice(&bytes[..copy_len]);
        
        fpga_bytes
    }
    
    /// Convert FPGA format back to BigUint
    fn fpga_format_to_bigint(&self, bytes: &[u8]) -> BigUint {
        BigUint::from_bytes_le(bytes)
    }
    
    /// Execute command on FPGA hardware
    fn execute_fpga_command(&self, command: &FpgaVdfCommand) -> Result<Vec<u8>, FpgaError> {
        // This would interface with the actual FPGA through DMA
        // For now, simulate the interface
        
        #[cfg(feature = "fpga-hardware")]
        {
            self.execute_fpga_dma(command)
        }
        
        #[cfg(not(feature = "fpga-hardware"))]
        {
            // Simulate FPGA execution for development
            self.simulate_fpga_execution(command)
        }
    }
    
    /// Simulate FPGA execution for development/testing
    fn simulate_fpga_execution(&self, command: &FpgaVdfCommand) -> Result<Vec<u8>, FpgaError> {
        match command.operation {
            VdfOperation::SequentialSquaring => {
                let base = self.fpga_format_to_bigint(&command.base);
                let modulus = self.fpga_format_to_bigint(&command.modulus);
                
                let mut result = base;
                for _ in 0..command.iterations {
                    result = (&result * &result) % &modulus;
                }
                
                Ok(self.bigint_to_fpga_format(&result))
            }
        }
    }
    
    /// Execute FPGA command via DMA (requires FPGA hardware)
    #[cfg(feature = "fpga-hardware")]
    fn execute_fpga_dma(&self, command: &FpgaVdfCommand) -> Result<Vec<u8>, FpgaError> {
        use std::fs::OpenOptions;
        use std::io::{Read, Write, Seek, SeekFrom};
        
        // Open FPGA device
        let device_path = format!("/dev/xdma{}_h2c_0", self.device_id);
        let mut fpga_device = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&device_path)
            .map_err(|_| FpgaError::DeviceNotFound)?;
        
        // Serialize command to FPGA format
        let command_bytes = self.serialize_fpga_command(command)?;
        
        // Write command to FPGA
        fpga_device.write_all(&command_bytes)
            .map_err(|_| FpgaError::WriteError)?;
        
        // Wait for completion and read result
        std::thread::sleep(Duration::from_micros(10)); // Estimated FPGA execution time
        
        let mut result_bytes = vec![0u8; 256];
        fpga_device.seek(SeekFrom::Start(0))
            .map_err(|_| FpgaError::ReadError)?;
        fpga_device.read_exact(&mut result_bytes)
            .map_err(|_| FpgaError::ReadError)?;
        
        Ok(result_bytes)
    }
    
    /// Serialize command for FPGA
    fn serialize_fpga_command(&self, command: &FpgaVdfCommand) -> Result<Vec<u8>, FpgaError> {
        let mut bytes = Vec::new();
        
        // Command header
        bytes.extend_from_slice(&(command.operation as u32).to_le_bytes());
        bytes.extend_from_slice(&command.iterations.to_le_bytes());
        
        // Base value (256 bytes)
        bytes.extend_from_slice(&command.base);
        
        // Modulus (256 bytes)  
        bytes.extend_from_slice(&command.modulus);
        
        Ok(bytes)
    }
    
    fn fiat_shamir_challenge(&self, input: &[u8], output: &BigUint, iterations: u64) -> BigUint {
        let mut hasher = Sha256::new();
        hasher.update(input);
        hasher.update(&output.to_bytes_be());
        hasher.update(&iterations.to_be_bytes());
        
        hash_to_prime(&hasher.finalize())
    }
}

impl VdfBackend for FpgaVdfBackend {
    fn prove(&self, input: &[u8], iterations: u64) -> VdfResult<VdfProof> {
        if input.is_empty() {
            return Err(VdfError::InvalidInput);
        }
        
        // Hash input to get base element
        let base = hash_to_prime(input);
        
        // Compute y = g^(2^T) mod N using FPGA acceleration
        let output = self.fpga_sequential_squaring(&base, iterations)?;
        
        // Generate Fiat-Shamir challenge
        let challenge = self.fiat_shamir_challenge(input, &output, iterations);
        
        // Compute proof (for now, use CPU - could be FPGA accelerated too).
        // (Fixes swapped modpow arguments and unbounded-T quotient.)
        let group = crate::posq::Group::new(self.params.modulus.clone());
        let proof = crate::posq::quotient_power(&group, &base, iterations, &challenge);
        
        Ok(VdfProof {
            input: input.to_vec(),
            output,
            proof,
            iterations,
        })
    }
    
    fn verify(&self, vdf_proof: &VdfProof) -> VdfResult<bool> {
        if vdf_proof.input.is_empty() {
            return Err(VdfError::InvalidInput);
        }
        
        // Use CPU verification for now (fast enough)
        let base = hash_to_prime(&vdf_proof.input);
        let challenge = self.fiat_shamir_challenge(
            &vdf_proof.input, 
            &vdf_proof.output, 
            vdf_proof.iterations
        );
        
        let two = BigUint::from(2u32);
        let r = two.modpow(&BigUint::from(vdf_proof.iterations), &challenge);
        
        let left_side = (vdf_proof.proof.modpow(&challenge, &self.params.modulus) 
                        * base.modpow(&r, &self.params.modulus)) 
                        % &self.params.modulus;
        
        Ok(left_side == vdf_proof.output)
    }
    
    fn estimated_time(&self, iterations: u64) -> Duration {
        if self.fpga_available {
            // FPGA can process much faster - estimated 50-100x speedup
            Duration::from_nanos((iterations as f64 * 30.0) as u64) // ~30ns per iteration
        } else {
            // Fallback to CPU timing
            Duration::from_nanos((iterations as f64 * 2200.0) as u64)
        }
    }
    
    fn name(&self) -> &'static str {
        if self.fpga_available {
            "FpgaVdfBackend"
        } else {
            "FpgaVdfBackend(CPU_Fallback)"
        }
    }
}

/// FPGA command structure
#[derive(Debug, Clone)]
struct FpgaVdfCommand {
    operation: VdfOperation,
    base: Vec<u8>,
    modulus: Vec<u8>,
    iterations: u64,
}

/// FPGA operations
#[derive(Debug, Clone, Copy)]
#[repr(u32)]
enum VdfOperation {
    SequentialSquaring = 1,
}

/// FPGA errors
#[derive(Debug)]
enum FpgaError {
    DeviceNotFound,
    WriteError,
    ReadError,
    InvalidResponse,
}

impl std::fmt::Display for FpgaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FpgaError::DeviceNotFound => write!(f, "FPGA device not found"),
            FpgaError::WriteError => write!(f, "Failed to write to FPGA device"),
            FpgaError::ReadError => write!(f, "Failed to read from FPGA device"),
            FpgaError::InvalidResponse => write!(f, "Invalid response from FPGA"),
        }
    }
}

impl std::error::Error for FpgaError {}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_fpga_backend_fallback() {
        // This will fall back to CPU since no FPGA is available in test environment
        let backend = FpgaVdfBackend::with_default_params().expect("Failed to create FPGA backend");
        
        let input = b"fpga test";
        let iterations = 10;
        
        let proof = backend.prove(input, iterations).expect("Failed to generate proof");
        let is_valid = backend.verify(&proof).expect("Failed to verify proof");
        
        assert!(is_valid, "FPGA backend proof should be valid");
        assert_eq!(proof.input, input);
        assert_eq!(proof.iterations, iterations);
    }
    
    #[test]
    fn test_bigint_fpga_conversion() {
        let backend = FpgaVdfBackend::with_default_params().expect("Failed to create backend");
        
        let value = BigUint::from(12345u32);
        let fpga_bytes = backend.bigint_to_fpga_format(&value);
        let recovered = backend.fpga_format_to_bigint(&fpga_bytes);
        
        assert_eq!(value, recovered);
    }
}
