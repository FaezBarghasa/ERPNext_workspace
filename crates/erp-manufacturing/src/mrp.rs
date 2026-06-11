use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::{BinaryHeap, HashMap};
use std::cmp::Ordering;
use chrono::{DateTime, Duration, Utc};
use tokio::sync::mpsc;
use crate::bom::BOMNode;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SalesForecast {
    pub item_id: String,
    pub qty: Decimal,
    pub target_date: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProductionPlan {
    pub item_id: String,
    pub qty: Decimal,
    pub target_date: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WarehouseStock {
    pub warehouse_id: String,
    pub item_id: String,
    pub available_qty: Decimal,
    pub reserved_qty: Decimal,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PurchaseOrder {
    pub item_id: String,
    pub qty: Decimal,
    pub expected_delivery: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkOrder {
    pub item_id: String,
    pub qty: Decimal,
    pub expected_delivery: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum OrderType {
    Purchase,
    WorkOrder,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannedOrder {
    pub item_id: String,
    pub qty: Decimal,
    pub order_type: OrderType,
    pub release_date: DateTime<Utc>,
    pub due_date: DateTime<Utc>,
}

#[derive(Clone, Debug)]
struct DemandEntry {
    item_id: String,
    qty: Decimal,
    date: DateTime<Utc>,
}

impl PartialEq for DemandEntry {
    fn eq(&self, other: &Self) -> bool {
        self.date == other.date && self.item_id == other.item_id
    }
}
impl Eq for DemandEntry {}

impl PartialOrd for DemandEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for DemandEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        // Min-heap order: earlier dates have higher priority
        other.date.cmp(&self.date)
            .then_with(|| self.item_id.cmp(&other.item_id))
    }
}

#[derive(Debug, Clone)]
struct SupplyEntry {
    date: DateTime<Utc>,
    item_id: String,
    qty: Decimal,
}

pub struct MRPEngine {
    pub lead_times: HashMap<String, u32>,          // Lead time in days
    pub min_order_qtys: HashMap<String, Decimal>,  // Minimum Order Quantity (MOQ)
    pub bom_registry: HashMap<String, BOMNode>,    // BOM definitions for manufactured items
}

impl MRPEngine {
    pub fn new(
        lead_times: HashMap<String, u32>,
        min_order_qtys: HashMap<String, Decimal>,
        bom_registry: HashMap<String, BOMNode>,
    ) -> Self {
        Self {
            lead_times,
            min_order_qtys,
            bom_registry,
        }
    }

    /// Run the time-phased MRP scheduling loop.
    pub fn plan(
        &self,
        forecasts: &[SalesForecast],
        production_plans: &[ProductionPlan],
        current_stock: &[WarehouseStock],
        existing_pos: &[PurchaseOrder],
        existing_wos: &[WorkOrder],
    ) -> Vec<PlannedOrder> {
        let mut planned_orders = Vec::new();

        // 1. Calculate starting on-hand inventory net balances
        let mut on_hand = HashMap::new();
        for stock in current_stock {
            let net_qty = stock.available_qty - stock.reserved_qty;
            *on_hand.entry(stock.item_id.clone()).or_insert(Decimal::ZERO) += net_qty;
        }

        // 2. Format existing supply entries and sort chronologically
        let mut supplies = Vec::new();
        for po in existing_pos {
            supplies.push(SupplyEntry {
                date: po.expected_delivery,
                item_id: po.item_id.clone(),
                qty: po.qty,
            });
        }
        for wo in existing_wos {
            supplies.push(SupplyEntry {
                date: wo.expected_delivery,
                item_id: wo.item_id.clone(),
                qty: wo.qty,
            });
        }
        supplies.sort_by_key(|s| s.date);

        // 3. Build the priority queue of demands (forecasts + plans)
        let mut demand_heap = BinaryHeap::new();
        for f in forecasts {
            demand_heap.push(DemandEntry {
                item_id: f.item_id.clone(),
                qty: f.qty,
                date: f.target_date,
            });
        }
        for p in production_plans {
            demand_heap.push(DemandEntry {
                item_id: p.item_id.clone(),
                qty: p.qty,
                date: p.target_date,
            });
        }

        let mut supply_idx = 0;

        // 4. Process demands chronologically
        while let Some(demand) = demand_heap.pop() {
            let item_id = &demand.item_id;

            // Apply all incoming supplies arriving on or before the demand date
            while supply_idx < supplies.len() && supplies[supply_idx].date <= demand.date {
                let supply = &supplies[supply_idx];
                *on_hand.entry(supply.item_id.clone()).or_insert(Decimal::ZERO) += supply.qty;
                supply_idx += 1;
            }

            let current_qty = on_hand.entry(item_id.clone()).or_insert(Decimal::ZERO);
            if *current_qty >= demand.qty {
                // Stock covers this demand
                *current_qty -= demand.qty;
            } else {
                // Shortage detected
                let shortage = demand.qty - *current_qty;
                *current_qty = Decimal::ZERO;

                // Adjust for Minimum Order Quantity (MOQ)
                let moq = self.min_order_qtys.get(item_id).copied().unwrap_or(Decimal::ZERO);
                let order_qty = if shortage < moq { moq } else { shortage };

                // Offset lead time to get the order release date
                let lead_days = self.lead_times.get(item_id).copied().unwrap_or(0);
                let due_date = demand.date;
                let release_date = due_date - Duration::days(lead_days as i64);

                let has_bom = self.bom_registry.contains_key(item_id);
                let order_type = if has_bom {
                    OrderType::WorkOrder
                } else {
                    OrderType::Purchase
                };

                let planned = PlannedOrder {
                    item_id: item_id.clone(),
                    qty: order_qty,
                    order_type: order_type.clone(),
                    release_date,
                    due_date,
                };
                planned_orders.push(planned);

                // Excess stock from MOQ is added to the inventory balance immediately
                let excess = order_qty - shortage;
                *current_qty += excess;

                // If manufactured, recursively explode the BOM and queue component demands at the release date
                if let Some(bom) = self.bom_registry.get(item_id) {
                    for comp in &bom.components {
                        let comp_demand_qty = order_qty * comp.qty;
                        demand_heap.push(DemandEntry {
                            item_id: comp.child_node.item_id.clone(),
                            qty: comp_demand_qty,
                            date: release_date,
                        });
                    }
                }
            }
        }

        planned_orders
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DemandSource {
    pub order_id: String,
    pub item_id: String,
    pub qty: Decimal,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MRPResult {
    pub item_id: String,
    pub shortage_qty: Decimal,
}

/// Legacy/concurrent shortage calculator.
pub async fn run_mrp_pipeline(
    demands: Vec<DemandSource>,
    stocks: HashMap<String, Decimal>,
) -> Vec<MRPResult> {
    let (tx, mut rx) = mpsc::channel(100);

    for demand in demands {
        let tx_clone = tx.clone();
        let available = stocks.get(&demand.item_id).copied().unwrap_or(Decimal::ZERO);

        tokio::spawn(async move {
            let shortage = if demand.qty > available {
                demand.qty - available
            } else {
                Decimal::ZERO
            };

            let _ = tx_clone.send(MRPResult {
                item_id: demand.item_id,
                shortage_qty: shortage,
            }).await;
        });
    }

    drop(tx);

    let mut results = Vec::new();
    while let Some(res) = rx.recv().await {
        results.push(res);
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    use chrono::TimeZone;
    use crate::bom::BOMComponent;

    #[tokio::test]
    async fn test_mrp_pipeline_shortage_calculation() {
        let demands = vec![
            DemandSource { order_id: "ORD-1".into(), item_id: "ITEM-A".into(), qty: dec!(100.0) },
            DemandSource { order_id: "ORD-2".into(), item_id: "ITEM-B".into(), qty: dec!(50.0) },
            DemandSource { order_id: "ORD-3".into(), item_id: "ITEM-A".into(), qty: dec!(20.0) },
        ];

        let mut stocks = HashMap::new();
        stocks.insert("ITEM-A".into(), dec!(80.0));
        stocks.insert("ITEM-B".into(), dec!(60.0));

        let results = run_mrp_pipeline(demands, stocks).await;

        assert_eq!(results.len(), 3);

        let a_shortages: Vec<_> = results.iter().filter(|r| r.item_id == "ITEM-A").collect();
        let b_shortages: Vec<_> = results.iter().filter(|r| r.item_id == "ITEM-B").collect();

        assert!(a_shortages.iter().any(|r| r.shortage_qty == dec!(20.0)));
        assert!(a_shortages.iter().any(|r| r.shortage_qty == dec!(0.0)));
        assert_eq!(b_shortages[0].shortage_qty, dec!(0.0));
    }

    #[test]
    fn test_time_phased_mrp_planning() {
        let t1 = Utc.with_ymd_and_hms(2026, 6, 10, 12, 0, 0).unwrap();
        let t2 = Utc.with_ymd_and_hms(2026, 6, 15, 12, 0, 0).unwrap();

        // Lead times: FG takes 5 days, raw material takes 2 days
        let mut lead_times = HashMap::new();
        lead_times.insert("FG-1".to_string(), 5);
        lead_times.insert("RM-A".to_string(), 2);

        // MOQ: RM-A has MOQ of 100
        let mut min_order_qtys = HashMap::new();
        min_order_qtys.insert("RM-A".to_string(), dec!(100));

        // BOM registry: FG-1 requires 2 of RM-A
        let mut bom_registry = HashMap::new();
        let raw_component = BOMComponent {
            child_node: BOMNode {
                item_id: "RM-A".to_string(),
                is_phantom: false,
                components: vec![],
                operations: vec![],
            },
            qty: dec!(2),
            rate: dec!(5),
        };
        let fg_bom = BOMNode {
            item_id: "FG-1".to_string(),
            is_phantom: false,
            components: vec![raw_component],
            operations: vec![],
        };
        bom_registry.insert("FG-1".to_string(), fg_bom);

        let engine = MRPEngine::new(lead_times, min_order_qtys, bom_registry);

        // Current Inventory: FG-1 has 10 units available, 2 reserved. Net = 8.
        let current_stock = vec![
            WarehouseStock {
                warehouse_id: "WH-1".to_string(),
                item_id: "FG-1".to_string(),
                available_qty: dec!(10),
                reserved_qty: dec!(2),
            },
        ];

        // Demand: Production Plan of 15 units of FG-1 due at t2 (June 15).
        // Net requirement for FG-1 = 15 - 8 = 7 units.
        // Due Date = June 15.
        // Lead Time offset = 5 days -> Release Date = June 10.
        // Since FG-1 is manufactured, it explodes BOM -> requires 7 * 2 = 14 units of RM-A due at June 10.
        // RM-A MOQ = 100 -> Planned PO for RM-A of 100 units due on June 10.
        // RM-A Lead Time offset = 2 days -> Release Date = June 8.
        let production_plans = vec![
            ProductionPlan {
                item_id: "FG-1".to_string(),
                qty: dec!(15),
                target_date: t2,
            },
        ];

        let planned = engine.plan(
            &[],
            &production_plans,
            &current_stock,
            &[],
            &[],
        );

        // Should have planned:
        // 1. Work Order for FG-1 (qty 7, due June 15, release June 10)
        // 2. Purchase Order for RM-A (qty 100, due June 10, release June 8)
        assert_eq!(planned.len(), 2);

        let fg_plan = planned.iter().find(|p| p.item_id == "FG-1").unwrap();
        assert_eq!(fg_plan.qty, dec!(7));
        assert_eq!(fg_plan.order_type, OrderType::WorkOrder);
        assert_eq!(fg_plan.due_date, t2);
        assert_eq!(fg_plan.release_date, t1);

        let rm_plan = planned.iter().find(|p| p.item_id == "RM-A").unwrap();
        assert_eq!(rm_plan.qty, dec!(100)); // rounded up to MOQ
        assert_eq!(rm_plan.order_type, OrderType::Purchase);
        assert_eq!(rm_plan.due_date, t1);
        assert_eq!(rm_plan.release_date, Utc.with_ymd_and_hms(2026, 6, 8, 12, 0, 0).unwrap());
    }
}
