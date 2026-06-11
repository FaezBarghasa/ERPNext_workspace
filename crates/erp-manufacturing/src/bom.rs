use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BOMComponent {
    pub child_node: BOMNode,
    pub qty: Decimal,
    pub rate: Decimal,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BOMOperation {
    pub operation_id: String,
    pub workstation: String,
    pub time_in_mins: Decimal,
    pub hour_rate: Decimal,        // Operating cost per hour (overhead, power, etc.)
    pub labor_rate: Decimal,       // Labor cost per hour
    pub machine_wear_rate: Decimal, // Machine depreciation/wear rate per hour
    pub batch_size: Decimal,       // Batch size this operation time applies to
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct BOMNode {
    pub item_id: String,
    pub is_phantom: bool,
    pub components: Vec<BOMComponent>,
    pub operations: Vec<BOMOperation>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BOMValuation {
    pub material_cost: Decimal,
    pub labor_cost: Decimal,
    pub machine_wear_cost: Decimal,
    pub operating_cost: Decimal,
    pub total_cost: Decimal,
}

/// Recursively calculates the multi-level valuation of a BOM, expanding phantom nodes or sub-assemblies.
///
/// Algorithmic Complexity: $O(V + E)$ where $V$ is number of components and operations in the DAG.
pub fn calculate_bom_valuation(node: &BOMNode, qty: Decimal) -> BOMValuation {
    let mut material_cost = Decimal::ZERO;
    let mut labor_cost = Decimal::ZERO;
    let mut machine_wear_cost = Decimal::ZERO;
    let mut operating_cost = Decimal::ZERO;

    // 1. Process components
    for comp in &node.components {
        let child_qty = comp.qty * qty;
        if comp.child_node.is_phantom || !comp.child_node.components.is_empty() {
            // Expand phantom or sub-assembly component inline
            let child_val = calculate_bom_valuation(&comp.child_node, child_qty);
            material_cost += child_val.material_cost;
            labor_cost += child_val.labor_cost;
            machine_wear_cost += child_val.machine_wear_cost;
            operating_cost += child_val.operating_cost;
        } else {
            // Standard raw material / leaf node
            material_cost += child_qty * comp.rate;
        }
    }

    // 2. Process routing operations
    for op in &node.operations {
        let batch_size = if op.batch_size.is_zero() { Decimal::ONE } else { op.batch_size };
        let operation_time_mins = (qty / batch_size) * op.time_in_mins;
        let operation_time_hours = operation_time_mins / Decimal::new(60, 0);

        let op_wear = operation_time_hours * op.machine_wear_rate;
        let op_labor = operation_time_hours * op.labor_rate;
        let op_operating = operation_time_hours * op.hour_rate;

        machine_wear_cost += op_wear;
        labor_cost += op_labor;
        operating_cost += op_operating;
    }

    let total_cost = material_cost + labor_cost + machine_wear_cost + operating_cost;

    BOMValuation {
        material_cost,
        labor_cost,
        machine_wear_cost,
        operating_cost,
        total_cost,
    }
}

/// Legacy/wrapper helper function to calculate the total cost of a BOM for quantity = 1.
pub fn calculate_bom_cost(node: &BOMNode) -> Decimal {
    calculate_bom_valuation(node, Decimal::ONE).total_cost
}

/// Dynamic BOM Tree Compiler
/// Resolves complex, deeply nested parts requirements recursively without database join issues.
pub struct BOMTreeCompiler {
    pub parent_item: String,
    pub max_depth: u32,
}

impl BOMTreeCompiler {
    pub fn new(parent_item: &str, max_depth: u32) -> Self {
        Self {
            parent_item: parent_item.to_string(),
            max_depth,
        }
    }

    /// Generates a SurrealQL query using graph traversal to fetch the entire BOM DAG.
    /// Uses the recursive `..=N` syntax in SurrealQL to traverse edges natively.
    pub fn compile_dag_query(&self) -> String {
        format!(
            "SELECT \n    id, \n    item_id, \n    is_phantom, \n    ->requires..={}->bom_node.* AS components, \n    ->has_operation->bom_operation.* AS operations \nFROM item:{} \nFETCH components, operations;",
            self.max_depth, self.parent_item
        )
    }

    /// Compiles a query to calculate raw material rollups directly in the database.
    /// Traverses the graph, filters out phantom assemblies, and groups by raw material.
    pub fn compile_flattened_requirements_query(&self, target_qty: Decimal) -> String {
        format!(
            "SELECT \n    math::sum(qty * {}) AS total_qty, \n    out AS raw_material_id \nFROM (\n    SELECT \n        ->requires..={}->(bom_node WHERE is_phantom = false) \n    FROM item:{}\n) \nGROUP BY raw_material_id;",
            target_qty, self.max_depth, self.parent_item
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_phantom_bom_cost() {
        // Child phantom: 2 * $5 component + $2 routing = $12 unit cost
        let child_comp = BOMComponent {
            child_node: BOMNode {
                item_id: "raw_material".to_string(),
                is_phantom: false,
                components: vec![],
                operations: vec![],
            },
            qty: Decimal::new(2, 0),
            rate: Decimal::new(5, 0),
        };
        let phantom_node = BOMNode {
            item_id: "phantom_assembly".to_string(),
            is_phantom: true,
            components: vec![child_comp],
            operations: vec![BOMOperation {
                operation_id: "op_phantom".to_string(),
                workstation: "bench_1".to_string(),
                time_in_mins: dec!(60), // 1 hour
                hour_rate: dec!(2),
                labor_rate: dec!(0),
                machine_wear_rate: dec!(0),
                batch_size: dec!(1),
            }],
        };

        // Parent node: 3 * phantom_assembly + $10 routing
        // Total cost = 3 * 12 + 10 = $46
        let parent_comp = BOMComponent {
            child_node: phantom_node,
            qty: Decimal::new(3, 0),
            rate: Decimal::ZERO, // Rate is ignored for phantom/sub-assembly nodes
        };

        let parent_node = BOMNode {
            item_id: "finished_product".to_string(),
            is_phantom: false,
            components: vec![parent_comp],
            operations: vec![BOMOperation {
                operation_id: "op_parent".to_string(),
                workstation: "bench_2".to_string(),
                time_in_mins: dec!(30), // 0.5 hour
                hour_rate: dec!(20),    // 0.5 * 20 = 10
                labor_rate: dec!(0),
                machine_wear_rate: dec!(0),
                batch_size: dec!(1),
            }],
        };

        let total_cost = calculate_bom_cost(&parent_node);
        assert_eq!(total_cost, Decimal::new(46, 0));
    }

    #[test]
    fn test_complex_bom_valuation() {
        // Raw material components
        let raw_a = BOMComponent {
            child_node: BOMNode::default(),
            qty: dec!(2.5),
            rate: dec!(10.0), // 25.0
        };
        let raw_b = BOMComponent {
            child_node: BOMNode::default(),
            qty: dec!(1.0),
            rate: dec!(15.0), // 15.0
        };

        // Sub-assembly with operations
        let sub_assembly = BOMNode {
            item_id: "sub_assembly".to_string(),
            is_phantom: false,
            components: vec![raw_a, raw_b],
            operations: vec![BOMOperation {
                operation_id: "op_sub".to_string(),
                workstation: "workstation_sub".to_string(),
                time_in_mins: dec!(120),       // 2 hours
                hour_rate: dec!(5.0),          // 10.0 operating
                labor_rate: dec!(12.5),        // 25.0 labor
                machine_wear_rate: dec!(2.5),  // 5.0 wear
                batch_size: dec!(1),
            }],
        };

        // One sub-assembly per parent finished product
        let parent_comp = BOMComponent {
            child_node: sub_assembly,
            qty: dec!(1.0),
            rate: dec!(0),
        };

        // Parent routing operations (processed in batches of 5 units)
        let parent_node = BOMNode {
            item_id: "fg_product".to_string(),
            is_phantom: false,
            components: vec![parent_comp],
            operations: vec![BOMOperation {
                operation_id: "op_parent".to_string(),
                workstation: "workstation_parent".to_string(),
                time_in_mins: dec!(60),        // 1 hour per batch
                hour_rate: dec!(30.0),         // 30.0 operating per hour
                labor_rate: dec!(40.0),        // 40.0 labor per hour
                machine_wear_rate: dec!(10.0), // 10.0 wear per hour
                batch_size: dec!(5),
            }],
        };

        // Let's calculate for 10 units of finished product
        // material_cost of 10 parent = 10 * (25.0 + 15.0) = 400.0
        // sub-assembly operations for 10 units = 10 * 2 hours = 20 hours
        //   operating = 20 * 5 = 100.0
        //   labor = 20 * 12.5 = 250.0
        //   wear = 20 * 2.5 = 50.0
        // parent operations for 10 units (batch size 5, so 2 batches) = 2 hours
        //   operating = 2 * 30 = 60.0
        //   labor = 2 * 40 = 80.0
        //   wear = 2 * 10 = 20.0
        //
        // Totals:
        //   material_cost = 400.0
        //   operating_cost = 100.0 + 60.0 = 160.0
        //   labor_cost = 250.0 + 80.0 = 330.0
        //   machine_wear_cost = 50.0 + 20.0 = 70.0
        //   total_cost = 400.0 + 160.0 + 330.0 + 70.0 = 960.0

        let valuation = calculate_bom_valuation(&parent_node, dec!(10));
        assert_eq!(valuation.material_cost, dec!(400.0));
        assert_eq!(valuation.operating_cost, dec!(160.0));
        assert_eq!(valuation.labor_cost, dec!(330.0));
        assert_eq!(valuation.machine_wear_cost, dec!(70.0));
        assert_eq!(valuation.total_cost, dec!(960.0));
    }
}
