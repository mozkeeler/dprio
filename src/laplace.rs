extern crate libm;
extern crate rand;

use rand::distributions::Standard;
use rand::rngs::ThreadRng;
use rand::Rng;

use crate::ParameterError;

use std::cmp;

// For the following on approximating a laplace distribution, see
// https://raw.githubusercontent.com/google/differential-privacy/74d5be96d4abe6820ef4838c00a1b78c72ae01af/common_docs/Secure_Noise_Generation.pdf

// Ported and adapted from
// https://github.com/google/differential-privacy/blob/74d5be96d4abe6820ef4838c00a1b78c72ae01af/java/main/com/google/privacy/differentialprivacy/LaplaceNoise.java
// https://github.com/google/differential-privacy/blob/74d5be96d4abe6820ef4838c00a1b78c72ae01af/java/main/com/google/privacy/differentialprivacy/SamplingUtil.java
// Original copyright notice:
//
// Copyright 2022 Google LLC
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//      http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

// Returns a f64 value in the range [0,1).
fn next_double(rng: &mut ThreadRng) -> f64 {
    rng.sample::<f64, Standard>(Standard)
}

// Draws a sample from the geometric distribution parameterized by p = 1 - e^(-lambda).
// Lambda must be greater than 2^(-59).
fn sample_geometric(rng: &mut ThreadRng, lambda: f64) -> Result<i64, ParameterError> {
    if lambda <= libm::exp2(-59.0_f64) {
        return Err(ParameterError);
    }

    // If the sample exceeds the maximum i64 value, the sample is truncated.
    if next_double(rng) > -1.0 * libm::expm1(-1.0_f64 * lambda * (i64::MAX as f64)) {
        return Ok(i64::MAX);
    }

    let mut left: i64 = 0;
    let mut right: i64 = i64::MAX;
    while left + 1 < right {
        let mid: i64 = libm::ceil(
            left as f64
                - (libm::log(0.5_f64) + libm::log1p(libm::exp(lambda * ((left - right) as f64))))
                    / lambda,
        ) as i64;
        let mid = cmp::min(cmp::max(mid, left + 1), right - 1);
        let q = libm::expm1(lambda * ((left - mid) as f64))
            / libm::expm1(lambda * ((left - right) as f64));
        if next_double(rng) <= q {
            right = mid;
        } else {
            left = mid;
        }
    }
    Ok(right)
}

fn sample_two_sided_geometric(rng: &mut ThreadRng, lambda: f64) -> Result<i64, ParameterError> {
    let mut geometric_sample = 0;
    let mut positive = false;
    while geometric_sample == 0 && !positive {
        geometric_sample = sample_geometric(rng, lambda)? - 1;
        positive = rng.sample::<bool, Standard>(Standard);
    }
    if positive {
        Ok(geometric_sample)
    } else {
        // geometric_sample can't be i64::MIN, so this won't wrap
        Ok(geometric_sample.wrapping_neg())
    }
}

pub fn noise(l1_sensitivity: f64, epsilon: f64) -> Result<i64, ParameterError> {
    // TODO: check parameters
    let granularity = get_granularity(l1_sensitivity, epsilon)?;
    let mut rng = rand::thread_rng();
    let two_sided_geometric_sample = sample_two_sided_geometric(
        &mut rng,
        granularity * epsilon / (l1_sensitivity + granularity),
    )?;
    if granularity <= 1.0_f64 {
        Ok((two_sided_geometric_sample as f64 * granularity).round() as i64)
    } else {
        Ok(two_sided_geometric_sample * granularity.trunc() as i64)
    }
}

// The granularity parameter is 2^40.
const GRANULARITY_PARAM: f64 = 1099511627776.0_f64;

// Returns the smallest power of 2 greater than or equal to x.
// x is a positive number less than or equal to 2^1023.
fn ceil_power_of_two(x: f64) -> Result<f64, ParameterError> {
    if x < 0.0_f64 {
        return Err(ParameterError);
    }
    if x > (1023.0_f64).exp2() {
        return Err(ParameterError);
    }
    let mut exponent = 0.0_f64;
    let mut val = exponent.exp2();
    while val < x {
        exponent += 1.0_f64;
        val = exponent.exp2();
    }
    Ok(val)
}

fn get_granularity(l1_sensitivity: f64, epsilon: f64) -> Result<f64, ParameterError> {
    Ok(ceil_power_of_two(l1_sensitivity / epsilon)? / GRANULARITY_PARAM)
}

// Given our analysis, the minimum number of bits required is ceil(log_2(r/lambda * 6 ln 10)), where
// r is the granularity (see `get_granularity`, and lambda is r * epsilon / (1l_sensitivity + r).
pub fn min_bits(l1_sensitivity: f64, epsilon: f64) -> Result<usize, ParameterError> {
    let granularity = get_granularity(l1_sensitivity, epsilon)?;
    let lambda = granularity * epsilon / (l1_sensitivity + granularity);
    Ok((6.0_f64 * 10.0_f64.ln() * granularity / lambda)
        .log2()
        .ceil() as usize)
}
