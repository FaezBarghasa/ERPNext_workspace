use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Lead {
    pub id: String,
    pub profile_completeness: Decimal,
    pub engagement_score: Decimal,
    pub fit_score: Decimal,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LeadDemographics {
    pub age: Decimal,
    pub annual_revenue: Decimal,
    pub email_opened_ratio: Decimal,
    pub web_visits_count: Decimal,
    pub country_score: Decimal,
    pub industry_score: Decimal,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScoringWeights {
    pub age_weight: Decimal,
    pub revenue_weight: Decimal,
    pub email_ratio_weight: Decimal,
    pub web_visits_weight: Decimal,
    pub country_weight: Decimal,
    pub industry_weight: Decimal,
    pub bias: Decimal,
}

/// Legacy lead scorer. Calculates a simple weighted average.
pub fn calculate_lead_score(
    lead: &Lead,
    profile_weight: Decimal,
    engagement_weight: Decimal,
    fit_weight: Decimal,
) -> Decimal {
    let score = (lead.profile_completeness * profile_weight)
        + (lead.engagement_score * engagement_weight)
        + (lead.fit_score * fit_weight);

    let total_weight = profile_weight + engagement_weight + fit_weight;
    if total_weight.is_zero() {
        return Decimal::ZERO;
    }

    score / total_weight
}

/// Natively calculates lead conversion ratings using a Logistic Regression model.
///
/// LaTeX:
/// $$z = \text{bias} + \sum (\text{weight}_i \times \text{feature}_i)$$
/// $$P(\text{conversion}) = \frac{1}{1 + e^{-z}}$$
pub fn calculate_lead_score_ml(
    demographics: &LeadDemographics,
    weights: &ScoringWeights,
) -> Decimal {
    let z = (demographics.age * weights.age_weight)
        + (demographics.annual_revenue * weights.revenue_weight)
        + (demographics.email_opened_ratio * weights.email_ratio_weight)
        + (demographics.web_visits_count * weights.web_visits_weight)
        + (demographics.country_score * weights.country_weight)
        + (demographics.industry_score * weights.industry_weight)
        + weights.bias;

    // Convert to f64 for transcendental exp() function, then convert back to Decimal
    let z_f64 = z.to_f64().unwrap_or(0.0);
    let sigmoid = 1.0 / (1.0 + (-z_f64).exp());

    Decimal::from_f64(sigmoid)
        .unwrap_or(Decimal::ZERO)
        .round_dp(4)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_legacy_lead_scoring() {
        let lead = Lead {
            id: "L-1".to_string(),
            profile_completeness: dec!(0.8),
            engagement_score: dec!(0.6),
            fit_score: dec!(0.9),
        };
        // (0.8 * 2 + 0.6 * 3 + 0.9 * 5) / 10 = (1.6 + 1.8 + 4.5) / 10 = 7.9 / 10 = 0.79
        let score = calculate_lead_score(&lead, dec!(2), dec!(3), dec!(5));
        assert_eq!(score, dec!(0.79));
    }

    #[test]
    fn test_logistic_regression_scoring() {
        let demographics = LeadDemographics {
            age: dec!(35),
            annual_revenue: dec!(150000),
            email_opened_ratio: dec!(0.85),
            web_visits_count: dec!(12),
            country_score: dec!(1.0),
            industry_score: dec!(0.90),
        };

        // We choose weights that will result in a clear probability output
        let weights = ScoringWeights {
            age_weight: dec!(0.01),       // 0.35
            revenue_weight: dec!(0.00001), // 1.50
            email_ratio_weight: dec!(1.5), // 1.275
            web_visits_weight: dec!(0.1),  // 1.20
            country_weight: dec!(0.5),     // 0.50
            industry_weight: dec!(0.4),    // 0.36
            bias: dec!(-5.185),            // bias to offset sum to exactly 0.0
        };

        // Sum = 0.35 + 1.50 + 1.275 + 1.20 + 0.50 + 0.36 - 5.185 = 5.185 - 5.185 = 0.0
        // sigmoid(0.0) = 1.0 / (1.0 + e^0) = 0.5000
        let prob = calculate_lead_score_ml(&demographics, &weights);
        assert_eq!(prob, dec!(0.5000));
    }
}
