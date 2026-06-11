use rust_decimal::Decimal;
use std::time::Instant;
use erp_manufacturing::bom::{calculate_bom_valuation, BOMComponent, BOMNode, BOMOperation};

fn create_level(depth: usize) -> BOMNode {
    if depth == 5 {
        // Raw material (leaf)
        BOMNode {
            item_id: format!("item_leaf_{}", depth),
            is_phantom: false,
            components: vec![],
            operations: vec![],
        }
    } else {
        // Sub-assembly
        let child = create_level(depth + 1);
        BOMNode {
            item_id: format!("item_level_{}", depth),
            is_phantom: depth % 2 == 1, // alternate phantom nodes
            components: vec![
                BOMComponent {
                    child_node: child,
                    qty: Decimal::new(2, 0), // 2 units of child
                    rate: Decimal::new(10, 0),
                },
                BOMComponent {
                    child_node: BOMNode {
                        item_id: format!("raw_mat_{}", depth),
                        is_phantom: false,
                        components: vec![],
                        operations: vec![],
                    },
                    qty: Decimal::new(5, 0),
                    rate: Decimal::new(5, 0), // 5 * 5 = 25 flat material cost
                }
            ],
            operations: vec![
                BOMOperation {
                    operation_id: format!("op_{}", depth),
                    workstation: "W1".to_string(),
                    time_in_mins: Decimal::new(10, 0),
                    hour_rate: Decimal::new(60, 0),
                    labor_rate: Decimal::new(30, 0),
                    machine_wear_rate: Decimal::new(18, 0),
                    batch_size: Decimal::ONE,
                }
            ],
        }
    }
}

fn main() {
    println!("Running bom_bench (BOM tree valuation benchmark)...");

    // 1. Build a 5-level deep BOM tree
    let root = create_level(1);

    // 2. Benchmark the recursive valuation
    let start = Instant::now();
    let num_iterations = 10_000;
    let mut total_cost = Decimal::ZERO;
    for _ in 0..num_iterations {
        let val = calculate_bom_valuation(&root, Decimal::ONE);
        total_cost = val.total_cost;
    }
    let duration = start.elapsed();

    println!("Success! Evaluated BOM valuation {} times.", num_iterations);
    println!("Total cost: {}", total_cost);
    println!("Total duration: {:?}", duration);
    println!("Mean time per valuation: {:?}", duration / num_iterations);
}
