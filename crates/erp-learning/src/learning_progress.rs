use rust_decimal::Decimal;
use rust_decimal_macros::dec;

pub fn calculate_progress(completed_lessons: u32, total_lessons: u32) -> Decimal {
    if total_lessons == 0 {
        return Decimal::ZERO;
    }
    Decimal::from(completed_lessons) / Decimal::from(total_lessons) * dec!(100)
}

pub fn verify_certification_eligibility(
    progress: Decimal,
    quiz_scores: &[Decimal],
    passing_score: Decimal,
) -> bool {
    if progress < dec!(100) {
        return false;
    }

    if quiz_scores.is_empty() {
        return false;
    }

    let mut total_score = Decimal::ZERO;
    for score in quiz_scores {
        total_score += score;
    }

    let average_score = total_score / Decimal::from(quiz_scores.len());
    average_score >= passing_score
}
