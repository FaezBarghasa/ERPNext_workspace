use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use crate::pricing_rules::{LineItem, PricingRule};
use chrono::{DateTime, Utc};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum TaxChargeType {
    Actual,
    OnNetTotal,
    OnPreviousRowAmount,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TaxRowTemplate {
    pub title: String,
    pub charge_type: TaxChargeType,
    pub rate_percentage: Decimal,
    pub compound: bool,
    pub item_wise_rates: HashMap<String, Decimal>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CurrencyContext {
    pub transaction_currency: String,
    pub company_currency: String,
    pub exchange_rate: Decimal,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TaxRow {
    pub title: String,
    pub tax_rate: Decimal,
    pub tax_amount: Decimal,
    pub base_tax_amount: Decimal,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DocumentTotals {
    pub net_total: Decimal,
    pub total_taxes: Decimal,
    pub grand_total: Decimal,
    pub base_net_total: Decimal,
    pub base_total_taxes: Decimal,
    pub base_grand_total: Decimal,
    pub taxes: Vec<TaxRow>,
}

/// Computes net totals, item-wise & compound taxes, and multi-currency conversions.
///
/// Algorithmic Complexity: $O(I \times T)$ where $I$ is number of items and $T$ is tax rows.
pub fn calculate_document_totals(
    items: &[LineItem],
    taxes: &[TaxRowTemplate],
    pricing: &[PricingRule],
    currency_ctx: &CurrencyContext,
    customer_class: Option<&str>,
    transaction_date: Option<DateTime<Utc>>,
) -> DocumentTotals {
    // 1. Calculate net rate per item after pricing rules, and sum net total
    let mut net_total = Decimal::ZERO;
    let mut item_net_amounts = Vec::new();
    for item in items {
        let rate = crate::pricing_rules::evaluate_pricing_rules(
            item,
            customer_class,
            transaction_date,
            pricing,
        );
        let item_net = item.qty * rate;
        net_total += item_net;
        item_net_amounts.push((item, item_net));
    }

    // 2. Initialize running taxable base
    let mut running_taxable_base = net_total;
    let mut tax_rows = Vec::new();
    let mut total_taxes = Decimal::ZERO;
    let mut previous_row_amount = Decimal::ZERO;

    for template in taxes {
        let mut row_amount = Decimal::ZERO;

        match template.charge_type {
            TaxChargeType::OnNetTotal => {
                let compounding_factor = if net_total.is_zero() {
                    Decimal::ONE
                } else {
                    running_taxable_base / net_total
                };

                for (item, item_net) in &item_net_amounts {
                    let rate = template.item_wise_rates.get(&item.id)
                        .or_else(|| template.item_wise_rates.get(&item.item_group))
                        .copied()
                        .unwrap_or(template.rate_percentage);
                    let item_base = *item_net * compounding_factor;
                    row_amount += item_base * (rate / Decimal::new(100, 0));
                }
            }
            TaxChargeType::OnPreviousRowAmount => {
                row_amount = previous_row_amount * (template.rate_percentage / Decimal::new(100, 0));
            }
            TaxChargeType::Actual => {
                row_amount = template.rate_percentage;
            }
        }

        total_taxes += row_amount;
        previous_row_amount = row_amount;

        if template.compound {
            running_taxable_base += row_amount;
        }

        let base_tax_amount = row_amount * currency_ctx.exchange_rate;
        tax_rows.push(TaxRow {
            title: template.title.clone(),
            tax_rate: template.rate_percentage,
            tax_amount: row_amount,
            base_tax_amount,
        });
    }

    let grand_total = net_total + total_taxes;

    DocumentTotals {
        net_total,
        total_taxes,
        grand_total,
        base_net_total: net_total * currency_ctx.exchange_rate,
        base_total_taxes: total_taxes * currency_ctx.exchange_rate,
        base_grand_total: grand_total * currency_ctx.exchange_rate,
        taxes: tax_rows,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_compound_and_item_wise_taxes() {
        let item1 = LineItem {
            id: "item_a".to_string(),
            item_group: "GroupA".to_string(),
            qty: dec!(2),
            base_rate: dec!(100), // Net = 200
        };
        let item2 = LineItem {
            id: "item_b".to_string(),
            item_group: "GroupB".to_string(),
            qty: dec!(1),
            base_rate: dec!(50), // Net = 50
        };

        // Tax 1: GST (10%), but item_a has an override of 5%
        let mut item_rates = HashMap::new();
        item_rates.insert("item_a".to_string(), dec!(5));
        let tax1 = TaxRowTemplate {
            title: "GST".to_string(),
            charge_type: TaxChargeType::OnNetTotal,
            rate_percentage: dec!(10), // default
            compound: true, // compound is true!
            item_wise_rates: item_rates,
        };

        // Tax 2: Service Tax (2% flat on the compounded base)
        let tax2 = TaxRowTemplate {
            title: "Service Tax".to_string(),
            charge_type: TaxChargeType::OnNetTotal,
            rate_percentage: dec!(2),
            compound: false,
            item_wise_rates: HashMap::new(),
        };

        let currency_ctx = CurrencyContext {
            transaction_currency: "USD".to_string(),
            company_currency: "EUR".to_string(),
            exchange_rate: dec!(0.9), // 1 USD = 0.9 EUR
        };

        let totals = calculate_document_totals(
            &[item1, item2],
            &[tax1, tax2],
            &[],
            &currency_ctx,
            None,
            None,
        );

        // Verification:
        // Net Total = 200 + 50 = 250 USD
        // Tax 1 (GST):
        //   item_a tax = 200 * 5% = 10 USD
        //   item_b tax = 50 * 10% = 5 USD
        //   Tax 1 Total = 15 USD.
        //   Since Tax 1 is compound, new running base = 250 + 15 = 265 USD
        //   Compounding Factor = 265 / 250 = 1.06
        // Tax 2 (Service Tax 2%):
        //   item_a base = 200 * 1.06 = 212. item_a tax = 212 * 2% = 4.24 USD
        //   item_b base = 50 * 1.06 = 53. item_b tax = 53 * 2% = 1.06 USD
        //   Tax 2 Total = 4.24 + 1.06 = 5.30 USD
        // Total Taxes = 15 + 5.30 = 20.30 USD
        // Grand Total = 250 + 20.30 = 270.30 USD
        // Base Grand Total = 270.30 * 0.9 = 243.27 EUR

        assert_eq!(totals.net_total, dec!(250));
        assert_eq!(totals.taxes[0].tax_amount, dec!(15));
        assert_eq!(totals.taxes[1].tax_amount, dec!(5.30));
        assert_eq!(totals.total_taxes, dec!(20.30));
        assert_eq!(totals.grand_total, dec!(270.30));
        assert_eq!(totals.base_grand_total, dec!(243.27));
    }
}
