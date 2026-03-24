/// Integer square root using Newton's method for i128.
/// Returns floor(sqrt(n)), or None if n < 0.
///
/// Used by:
/// - ReputationRegistry: level = sqrt(overall_score / 100)
/// - GovernanceVoting: QF formula sqrt of donation amounts
pub fn int_sqrt_i128(n: i128) -> Option<i128> {
    if n < 0 {
        return None;
    }
    if n == 0 {
        return Some(0);
    }
    if n <= 3 {
        return Some(1);
    }

    let mut x = n;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    Some(x)
}

/// Calculate fee from a gross amount and basis points.
/// Returns the fee amount, or None on overflow. bps = 500 means 5%.
///
/// fee = gross * bps / 10_000
pub fn calculate_fee_bps(gross: i128, bps: u32) -> Option<i128> {
    if bps == 0 || gross <= 0 {
        return Some(0);
    }
    gross.checked_mul(bps as i128)?.checked_div(10_000)
}

/// Split a fee into treasury and insurance portions.
/// insurance_bps is applied to the fee (not the gross).
/// Returns (treasury_cut, insurance_cut), or None on overflow.
///
/// Example: fee=500, insurance_bps=1000 (10% of fee)
///   insurance = 50, treasury = 450
pub fn split_fee(fee: i128, insurance_bps: u32) -> Option<(i128, i128)> {
    let insurance = calculate_fee_bps(fee, insurance_bps)?;
    let treasury = fee.checked_sub(insurance)?;
    Some((treasury, insurance))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sqrt_known_values() {
        assert_eq!(int_sqrt_i128(0), Some(0));
        assert_eq!(int_sqrt_i128(1), Some(1));
        assert_eq!(int_sqrt_i128(4), Some(2));
        assert_eq!(int_sqrt_i128(9), Some(3));
        assert_eq!(int_sqrt_i128(10), Some(3)); // floor
        assert_eq!(int_sqrt_i128(100), Some(10));
        assert_eq!(int_sqrt_i128(1_000_000), Some(1_000));
        // Large value for QF: 10^18 scaled
        assert_eq!(int_sqrt_i128(1_000_000_000_000_000_000), Some(1_000_000_000));
    }

    #[test]
    fn test_fee_calculation() {
        // 5% of 10,000
        assert_eq!(calculate_fee_bps(10_000, 500), Some(500));
        // 3% of 50,000
        assert_eq!(calculate_fee_bps(50_000, 300), Some(1_500));
        // 4% of 20,000
        assert_eq!(calculate_fee_bps(20_000, 400), Some(800));
        // 0 bps
        assert_eq!(calculate_fee_bps(10_000, 0), Some(0));
        // 0 amount
        assert_eq!(calculate_fee_bps(0, 500), Some(0));
    }

    #[test]
    fn test_fee_split() {
        // 10% of fee goes to insurance
        let (treasury, insurance) = split_fee(500, 1000).unwrap();
        assert_eq!(treasury, 450);
        assert_eq!(insurance, 50);
    }

    #[test]
    fn test_sqrt_negative() {
        assert_eq!(int_sqrt_i128(-1), None);
    }
}
