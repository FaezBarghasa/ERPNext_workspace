use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use rhai::{Engine, Scope};
use erp_accounting::posting::GLEntry;
use erp_accounting::LedgerError;
use surrealdb::types::RecordId;
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use tokio::task::JoinSet;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SalaryComponent {
    pub name: String,
    pub formula: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SalaryStructure {
    pub base_pay: Decimal,
    pub earnings: Vec<SalaryComponent>,
    pub deductions: Vec<SalaryComponent>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TaxSlab {
    pub min_amount: Decimal,
    pub max_amount: Option<Decimal>,
    pub rate: Decimal,        // e.g. 0.10 for 10%
    pub flat_amount: Decimal, // flat addition for this slab
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SalarySlipResult {
    pub employee_id: String,
    pub base_pay: Decimal,
    pub gross_pay: Decimal,
    pub total_deductions: Decimal,
    pub net_pay: Decimal,
    pub earnings: HashMap<String, Decimal>,
    pub deductions: HashMap<String, Decimal>,
}

#[derive(thiserror::Error, Debug)]
pub enum PayrollError {
    #[error("Evaluation error in formula for component '{0}': {1}")]
    Evaluation(String, String),
    #[error("Accounting integration error: {0}")]
    Ledger(#[from] LedgerError),
}

/// Calculate progressive tax over structured tax slabs.
pub fn calculate_progressive_tax(income: Decimal, slabs: &[TaxSlab]) -> Decimal {
    let mut tax = Decimal::ZERO;
    for slab in slabs {
        if income > slab.min_amount {
            let max = slab.max_amount.unwrap_or(income);
            let applicable_income = if income > max { max } else { income } - slab.min_amount;
            tax += (applicable_income * slab.rate) + slab.flat_amount;
        }
    }
    tax
}

/// Evaluates employee salary structures, parses component formulas dynamically using Rhai, and subtracts progressive tax.
pub fn compute_salary_slip_dynamic(
    employee_id: String,
    variables: &HashMap<String, Decimal>,
    structure: &SalaryStructure,
    tax_slabs: &[TaxSlab],
) -> Result<SalarySlipResult, PayrollError> {
    let engine = Engine::new();
    let mut scope = Scope::new();

    // 1. Load base pay and variables into the Rhai scope as f64 for standard math evaluation
    let base_f64 = structure.base_pay.to_f64().unwrap_or(0.0);
    scope.push("base", base_f64);

    for (name, val) in variables {
        let val_f64 = val.to_f64().unwrap_or(0.0);
        scope.push(name.clone(), val_f64);
    }

    // 2. Evaluate earning components
    let mut earnings_map = HashMap::new();
    let mut gross_pay = structure.base_pay;

    for earning in &structure.earnings {
        let result_f64 = engine.eval_with_scope::<f64>(&mut scope, &earning.formula)
            .map_err(|e| PayrollError::Evaluation(earning.name.clone(), e.to_string()))?;

        let result_dec = Decimal::from_f64(result_f64)
            .unwrap_or(Decimal::ZERO)
            .round_dp(2);

        earnings_map.insert(earning.name.clone(), result_dec);
        gross_pay += result_dec;

        // Push computed value back to scope for dependent component formulas
        scope.push(earning.name.clone(), result_f64);
    }

    // Update gross pay in the scope
    let gross_f64 = gross_pay.to_f64().unwrap_or(0.0);
    scope.push("gross_pay", gross_f64);

    // 3. Evaluate deduction components
    let mut deductions_map = HashMap::new();
    let mut total_deductions = Decimal::ZERO;

    for deduction in &structure.deductions {
        let result_f64 = engine.eval_with_scope::<f64>(&mut scope, &deduction.formula)
            .map_err(|e| PayrollError::Evaluation(deduction.name.clone(), e.to_string()))?;

        let result_dec = Decimal::from_f64(result_f64)
            .unwrap_or(Decimal::ZERO)
            .round_dp(2);

        deductions_map.insert(deduction.name.clone(), result_dec);
        total_deductions += result_dec;

        scope.push(deduction.name.clone(), result_f64);
    }

    // 4. Evaluate progressive income tax and append to deductions
    let income_tax = calculate_progressive_tax(gross_pay, tax_slabs).round_dp(2);
    if !income_tax.is_zero() {
        deductions_map.insert("Income Tax".to_string(), income_tax);
        total_deductions += income_tax;
    }

    let net_pay = gross_pay - total_deductions;

    Ok(SalarySlipResult {
        employee_id,
        base_pay: structure.base_pay,
        gross_pay,
        total_deductions,
        net_pay,
        earnings: earnings_map,
        deductions: deductions_map,
    })
}

/// Post double-entry postings for a batch of processed salary slips.
pub fn post_payroll_batch(
    slips: &[SalarySlipResult],
    bank_account: RecordId,
    expense_account: RecordId,
    tax_account: RecordId,
    voucher_type: &str,
    voucher_no: &str,
) -> Result<Vec<GLEntry>, PayrollError> {
    let mut total_gross = Decimal::ZERO;
    let mut total_deductions = Decimal::ZERO;
    let mut total_net = Decimal::ZERO;

    for slip in slips {
        total_gross += slip.gross_pay;
        total_deductions += slip.total_deductions;
        total_net += slip.net_pay;
    }

    // Debit Salary Expense (total gross), Credit Tax Payable (total deductions) & Cash/Bank (total net)
    let entries = vec![
        GLEntry {
            account: expense_account,
            debit: total_gross,
            credit: Decimal::ZERO,
            voucher_type: voucher_type.to_string(),
            voucher_no: voucher_no.to_string(),
            cost_center: None,
        },
        GLEntry {
            account: tax_account,
            debit: Decimal::ZERO,
            credit: total_deductions,
            voucher_type: voucher_type.to_string(),
            voucher_no: voucher_no.to_string(),
            cost_center: None,
        },
        GLEntry {
            account: bank_account,
            debit: Decimal::ZERO,
            credit: total_net,
            voucher_type: voucher_type.to_string(),
            voucher_no: voucher_no.to_string(),
            cost_center: None,
        },
    ];

    // Verify double-entry balancing rules
    erp_accounting::posting::validate_and_compile_transaction(&entries)?;

    Ok(entries)
}

/// Run the time-phased concurrent payroll batch calculator over the employee roster.
pub async fn run_concurrent_payroll_batch(
    db: &surrealdb::Surreal<surrealdb::engine::any::Any>,
    employee_ids: Vec<String>,
    variables_map: HashMap<String, HashMap<String, Decimal>>,
    structure: SalaryStructure,
    tax_slabs: Vec<TaxSlab>,
) -> Result<Vec<SalarySlipResult>, PayrollError> {
    let mut join_set: JoinSet<Result<SalarySlipResult, PayrollError>> = JoinSet::new();

    for emp_id in employee_ids {
        let vars = variables_map.get(&emp_id).cloned().unwrap_or_default();
        let struct_clone = structure.clone();
        let slabs_clone = tax_slabs.clone();

        join_set.spawn(async move {
            let slip = compute_salary_slip_dynamic(emp_id, &vars, &struct_clone, &slabs_clone)?;
            Ok::<SalarySlipResult, PayrollError>(slip)
        });
    }

    let mut results = Vec::with_capacity(join_set.len());
    while let Some(res) = join_set.join_next().await {
        match res {
            Ok(inner_res) => {
                let slip = inner_res?;
                results.push(slip);
            }
            Err(join_err) => {
                return Err(PayrollError::Evaluation(
                    "Concurrent task execution failed".to_string(),
                    join_err.to_string(),
                ));
            }
        }
    }

    let mut query_parts = Vec::new();
    query_parts.push("BEGIN TRANSACTION;".to_string());

    for slip in &results {
        let slip_json = serde_json::to_string(slip)
            .map_err(|e| PayrollError::Evaluation("Serialization error".to_string(), e.to_string()))?;
        query_parts.push(format!(
            "CREATE tabSalarySlip CONTENT {};",
            slip_json
        ));
    }

    query_parts.push("COMMIT TRANSACTION;".to_string());
    let full_query = query_parts.join("\n");

    db.query(&full_query)
        .await
        .map_err(|e| PayrollError::Ledger(LedgerError::Database(e.to_string())))?
        .check()
        .map_err(|e| PayrollError::Ledger(LedgerError::Database(e.to_string())))?;

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_progressive_tax_calculation() {
        let slabs = vec![
            TaxSlab {
                min_amount: dec!(10000.00),
                max_amount: Some(dec!(20000.00)),
                rate: dec!(0.10),
                flat_amount: dec!(0.00),
            },
            TaxSlab {
                min_amount: dec!(20000.00),
                max_amount: None,
                rate: dec!(0.20),
                flat_amount: dec!(1000.00), // flat 10% from previous slab
            },
        ];

        // Income of 15000 -> tax is (15000-10000)*0.10 = 500
        assert_eq!(calculate_progressive_tax(dec!(15000.00), &slabs), dec!(500.00));

        // Income of 25000 -> tax is 1000 flat + (25000-20000)*0.20 + 1000 (from first slab) = 3000
        // Wait: applicable_income in first slab = min(25000, 20000) - 10000 = 10000 * 0.10 = 1000.
        // Second slab = (25000 - 20000)*0.20 + 1000 flat = 1000 + 1000 = 2000.
        // Total = 1000 + 2000 = 3000.
        assert_eq!(calculate_progressive_tax(dec!(25000.00), &slabs), dec!(3000.00));
    }

    #[test]
    fn test_dynamic_salary_evaluation() {
        let structure = SalaryStructure {
            base_pay: dec!(5000.00),
            earnings: vec![
                SalaryComponent {
                    name: "HRA".to_string(),
                    formula: "base * 0.10".to_string(), // 500
                },
                SalaryComponent {
                    name: "Overtime".to_string(),
                    formula: "overtime_hours * 25.0".to_string(), // 10 * 25 = 250
                },
            ],
            deductions: vec![
                SalaryComponent {
                    name: "Provident Fund".to_string(),
                    formula: "base * 0.05".to_string(), // 250
                },
            ],
        };

        let mut variables = HashMap::new();
        variables.insert("overtime_hours".to_string(), dec!(10));

        let tax_slabs = vec![];

        let result = compute_salary_slip_dynamic(
            "EMP-001".to_string(),
            &variables,
            &structure,
            &tax_slabs,
        ).unwrap();

        assert_eq!(result.gross_pay, dec!(5750.00));
        assert_eq!(result.total_deductions, dec!(250.00));
        assert_eq!(result.net_pay, dec!(5500.00));
        assert_eq!(result.earnings.get("HRA").copied(), Some(dec!(500.00)));
        assert_eq!(result.earnings.get("Overtime").copied(), Some(dec!(250.00)));
        assert_eq!(result.deductions.get("Provident Fund").copied(), Some(dec!(250.00)));
    }

    #[test]
    fn test_payroll_batch_gl_posting() {
        let slip1 = SalarySlipResult {
            employee_id: "EMP-1".to_string(),
            base_pay: dec!(4000),
            gross_pay: dec!(4500),
            total_deductions: dec!(500),
            net_pay: dec!(4000),
            earnings: HashMap::new(),
            deductions: HashMap::new(),
        };

        let slip2 = SalarySlipResult {
            employee_id: "EMP-2".to_string(),
            base_pay: dec!(5000),
            gross_pay: dec!(5500),
            total_deductions: dec!(800),
            net_pay: dec!(4700),
            earnings: HashMap::new(),
            deductions: HashMap::new(),
        };

        let bank_acc = RecordId::parse_simple("account:bank").unwrap();
        let expense_acc = RecordId::parse_simple("account:salary_expense").unwrap();
        let tax_acc = RecordId::parse_simple("account:tax_payable").unwrap();

        let postings = post_payroll_batch(
            &[slip1, slip2],
            bank_acc,
            expense_acc,
            tax_acc,
            "Journal Entry",
            "JV-2026-001",
        ).unwrap();

        assert_eq!(postings.len(), 3);
        // Expense Debit: 4500 + 5500 = 10000
        assert_eq!(postings[0].debit, dec!(10000));
        // Tax Payable Credit: 500 + 800 = 1300
        assert_eq!(postings[1].credit, dec!(1300));
        // Bank Credit: 4000 + 4700 = 8700
        assert_eq!(postings[2].credit, dec!(8700));
    }

    #[tokio::test]
    async fn test_concurrent_payroll_batch() {
        let db = surrealdb::engine::any::connect("mem://").await.unwrap();
        db.use_ns("test").use_db("test").await.unwrap();

        let structure = SalaryStructure {
            base_pay: dec!(5000.00),
            earnings: vec![
                SalaryComponent {
                    name: "HRA".to_string(),
                    formula: "base * 0.10".to_string(),
                },
            ],
            deductions: vec![],
        };

        let employee_ids = vec!["EMP-1".to_string(), "EMP-2".to_string()];
        let mut variables_map = HashMap::new();
        variables_map.insert("EMP-1".to_string(), HashMap::new());
        variables_map.insert("EMP-2".to_string(), HashMap::new());

        let slips = run_concurrent_payroll_batch(&db, employee_ids, variables_map, structure, vec![]).await.unwrap();
        assert_eq!(slips.len(), 2);
        assert_eq!(slips[0].gross_pay, dec!(5500.00));
        assert_eq!(slips[1].gross_pay, dec!(5500.00));
    }
}
