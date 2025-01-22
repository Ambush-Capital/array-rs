use crate::kamino::utils::fraction::Fraction;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalculateBorrowResult {
    pub borrow_amount_f: Fraction,
    pub receive_amount: u64,
    pub borrow_fee: u64,
    pub referrer_fee: u64,
}

#[derive(Debug)]
pub struct CalculateRepayResult {
    pub settle_amount_f: Fraction,
    pub repay_amount: u64,
}