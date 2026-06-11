use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PricingRule {
    pub id: String,
    pub item_code: Option<String>,
    pub item_group: Option<String>,
    pub customer_class: Option<String>,
    pub min_qty: Decimal,
    pub max_qty: Option<Decimal>,
    pub valid_from: Option<DateTime<Utc>>,
    pub valid_upto: Option<DateTime<Utc>>,
    pub discount_percentage: Decimal,
    pub discount_amount: Decimal,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LineItem {
    pub id: String,
    pub item_group: String,
    pub qty: Decimal,
    pub base_rate: Decimal,
}

/// Evaluates a list of pricing rules against a given line item under a trade context.
///
/// Algorithmic Complexity: $O(R)$ where $R$ is the number of pricing rules.
pub fn evaluate_pricing_rules(
    item: &LineItem,
    customer_class: Option<&str>,
    transaction_date: Option<DateTime<Utc>>,
    rules: &[PricingRule],
) -> Decimal {
    let mut current_rate = item.base_rate;

    for rule in rules {
        // 1. Check item identifier match
        if rule.item_code.as_ref().is_some_and(|code| code != &item.id) {
            continue;
        }

        // 2. Check item group match
        if rule.item_group.as_ref().is_some_and(|group| group != &item.item_group) {
            continue;
        }

        // 3. Check customer class match
        if rule.customer_class.as_ref().is_some_and(|class| Some(class.as_str()) != customer_class) {
            continue;
        }

        // 4. Check quantity limits
        if item.qty < rule.min_qty {
            continue;
        }
        if rule.max_qty.is_some_and(|max| item.qty > max) {
            continue;
        }

        // 5. Check promotion validity dates
        if let Some(tx_date) = transaction_date {
            if rule.valid_from.is_some_and(|from| tx_date < from) {
                continue;
            }
            if rule.valid_upto.is_some_and(|upto| tx_date > upto) {
                continue;
            }
        }

        // Rule matches! Apply discount percentage
        if rule.discount_percentage > Decimal::ZERO {
            let discount = current_rate * (rule.discount_percentage / Decimal::new(100, 0));
            current_rate -= discount;
        }

        // Apply discount flat amount
        if rule.discount_amount > Decimal::ZERO {
            current_rate -= rule.discount_amount;
        }
    }

    if current_rate.is_sign_negative() {
        Decimal::ZERO
    } else {
        current_rate
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use rust_decimal_macros::dec;

    #[test]
    fn test_pricing_rule_evaluation() {
        let item = LineItem {
            id: "laptop_01".to_string(),
            item_group: "Electronics".to_string(),
            qty: dec!(5),
            base_rate: dec!(1000),
        };

        // Rule 1: matches group and min_qty, discount 10%
        let rule1 = PricingRule {
            id: "rule_1".to_string(),
            item_code: None,
            item_group: Some("Electronics".to_string()),
            customer_class: None,
            min_qty: dec!(2),
            max_qty: Some(dec!(10)),
            valid_from: None,
            valid_upto: None,
            discount_percentage: dec!(10),
            discount_amount: dec!(0),
        };

        // Rule 2: matches customer class, flat discount $50
        let rule2 = PricingRule {
            id: "rule_2".to_string(),
            item_code: None,
            item_group: None,
            customer_class: Some("VIP".to_string()),
            min_qty: dec!(1),
            max_qty: None,
            valid_from: None,
            valid_upto: None,
            discount_percentage: dec!(0),
            discount_amount: dec!(50),
        };

        // Rule 3: expired promo, should be ignored
        let expired_time = Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap();
        let rule3 = PricingRule {
            id: "rule_3".to_string(),
            item_code: None,
            item_group: None,
            customer_class: None,
            min_qty: dec!(1),
            max_qty: None,
            valid_from: Some(expired_time),
            valid_upto: Some(expired_time),
            discount_percentage: dec!(50),
            discount_amount: dec!(0),
        };

        let tx_time = Utc.with_ymd_and_hms(2026, 6, 10, 12, 0, 0).unwrap();
        let rules = vec![rule1, rule2, rule3];

        let final_rate = evaluate_pricing_rules(&item, Some("VIP"), Some(tx_time), &rules);
        // Step 1: base = 1000
        // Step 2: rule1 applies: 1000 - 100 = 900
        // Step 3: rule2 applies: 900 - 50 = 850
        // Step 4: rule3 ignored (expired)
        // Final: 850
        assert_eq!(final_rate, dec!(850));
    }
}
