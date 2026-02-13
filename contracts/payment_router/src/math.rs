pub fn calculate_fee(amount: i128, fee_bps: u32) -> i128 {
    (amount * fee_bps as i128) / 10000
}

pub fn calculate_portions(total_fee: i128, insurance_bps: u32) -> (i128, i128) {
    let insurance_portion = (total_fee * insurance_bps as i128) / 10000;
    let treasury_portion = total_fee - insurance_portion;
    (insurance_portion, treasury_portion)
}

pub fn calculate_net_amount(gross_amount: i128, total_fee: i128) -> i128 {
    gross_amount - total_fee
}
