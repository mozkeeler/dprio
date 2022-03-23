extern crate byteorder;
extern crate rand;
extern crate sha2;

use byteorder::{NetworkEndian, WriteBytesExt};
use rand::distributions::{Distribution, Uniform};
use sha2::{Digest, Sha256};

pub struct Commitment {
    n: u64,
    p: u64,
}

impl Commitment {
    pub fn new(n: u64) -> Commitment {
        let factor = u64::MAX / n;
        let between = Uniform::new_inclusive(0, n * factor);
        let mut rng = rand::thread_rng();
        Commitment {
            n,
            p: between.sample(&mut rng),
        }
    }

    fn new_with_p(n: u64, p: u64) -> Commitment {
        Commitment { n, p }
    }

    pub fn commit(&self) -> ClosedCommitment {
        let mut buf = Vec::with_capacity(std::mem::size_of::<u64>());
        buf.write_u64::<NetworkEndian>(self.p).unwrap();
        let hash = Sha256::digest(&buf);
        ClosedCommitment::new(self.n, hash.to_vec())
    }

    pub fn publish(&self) -> u64 {
        self.p
    }
}

pub struct ClosedCommitment {
    n: u64,
    hash: Vec<u8>,
}

impl ClosedCommitment {
    pub fn new(n: u64, hash: Vec<u8>) -> ClosedCommitment {
        ClosedCommitment { n, hash }
    }

    pub fn validate(&self, p: u64) -> Result<OpenedCommitment, CommitmentError> {
        let commitment = Commitment::new_with_p(self.n, p);
        let hash = commitment.commit().hash;
        if hash == self.hash {
            Ok(OpenedCommitment::new(self.n, p))
        } else {
            Err(CommitmentError::HashMismatch)
        }
    }
}

pub struct OpenedCommitment {
    n: u64,
    p: u64,
}

impl OpenedCommitment {
    fn new(n: u64, p: u64) -> OpenedCommitment {
        OpenedCommitment { n, p }
    }

    // TODO: how to make this anything that can iterate over OpenedCommitments?
    pub fn gather(opened_commitments: &[OpenedCommitment]) -> Result<u64, CommitmentError> {
        let mut sum: u128 = 0;
        let mut n: Option<u64> = None;
        for opened_commitment in opened_commitments {
            if n.is_none() {
                n.replace(opened_commitment.n);
            }
            if *n.as_ref().unwrap() != opened_commitment.n {
                return Err(CommitmentError::CorpusSizeMismatch);
            }
            sum += opened_commitment.p as u128;
        }
        let n = match n {
            Some(n) => n,
            None => {
                return Err(CommitmentError::EmptyCorpus);
            }
        };
        Ok((sum % n as u128) as u64)
    }
}

#[derive(Debug)]
pub enum CommitmentError {
    HashMismatch,
    CorpusSizeMismatch,
    EmptyCorpus,
}

#[derive(Debug)]
pub struct ParameterError;

// For the following on approximating a laplace distribution, see
// https://raw.githubusercontent.com/google/differential-privacy/main/common_docs/Secure_Noise_Generation.pdf

fn k(epsilon: f64) -> Result<i32, ParameterError> {
    // epsilon can't be too small, or k will be too large
    if epsilon < 1.0e-32_f64 {
        return Err(ParameterError);
    }
    let addition = 1.0_f64 + (2.0_f64 / epsilon).log2();
    assert!(addition < 107.0_f64);
    Ok(10 + (addition.trunc() as i32))
}

fn r(delta: f64, epsilon: f64) -> Result<f64, ParameterError> {
    if epsilon <= 0.0_f64 || delta < 0.0_f64 {
        return Err(ParameterError);
    }
    let k = k(epsilon)?;
    let minimum = (delta / epsilon) * 2.0_f64.powi(k);
    let mut minimum = (minimum.trunc() as u64) + 1;
    let mut power_of_2: u64 = 1;
    while minimum > 0 {
        minimum >>= 1;
        power_of_2 <<= 1;
    }
    Ok(power_of_2 as f64)
}

fn round_to_nearest_multiple(delta: f64, r: f64) -> f64 {
    let factor = (delta / r).ceil();
    r * factor
}

pub fn laplace(delta: f64, epsilon: f64) -> Result<u64, ParameterError> {
    if delta <= 0.0_f64 {
        return Err(ParameterError);
    }
    let r = r(delta, epsilon)?;
    // delta_subscript_r is basically delta rounded to the nearest multiple of r.
    let delta_subscript_r = round_to_nearest_multiple(delta, r);
    // Select i with probability proportional to exp(-|i| * r * epsilon / delta_subscript_r),
    // where i is 0 or 1 for the purposes of dprio.
    let proportional_prob_0 = 1.0_f64;
    let proportional_prob_1 = (-1.0_f64 * r * epsilon / delta_subscript_r).exp();
    let total = proportional_prob_0 + proportional_prob_1;
    let prob_0 = proportional_prob_0 / total;
    let sampler = Uniform::new(0.0_f64, 1.0_f64);
    let mut rng = rand::thread_rng();
    let sample = sampler.sample(&mut rng);
    if sample <= prob_0 {
        Ok(0)
    } else {
        Ok(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_commitment() {
        let n = 162_564_322;
        let commitments = vec![Commitment::new(n), Commitment::new(n), Commitment::new(n)];
        let closed_commitments: Vec<ClosedCommitment> =
            commitments.iter().map(|c| c.commit()).collect();
        let published_values: Vec<u64> = commitments.iter().map(|c| c.publish()).collect();
        let opened_commitments: Vec<OpenedCommitment> = closed_commitments
            .iter()
            .zip(published_values.iter())
            .map(|(closed_commitment, p)| {
                let result = closed_commitment.validate(*p);
                assert!(result.is_ok());
                result.unwrap()
            })
            .collect();
        let result = OpenedCommitment::gather(&opened_commitments);
        assert!(result.is_ok());
        let index = result.unwrap();
        assert!(index < n);
    }
}
