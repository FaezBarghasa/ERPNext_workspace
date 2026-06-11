use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum PipelineState {
    Lead,
    Qualified,
    ProposalSent,
    Negotiation,
    ClosedWon,
    ClosedLost,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PipelineStatus {
    pub current_state: PipelineState,
}

impl PipelineStatus {
    pub fn transition(&mut self, next_state: PipelineState) -> Result<(), &'static str> {
        match (&self.current_state, &next_state) {
            (PipelineState::Lead, PipelineState::Qualified) => {
                self.current_state = next_state;
                Ok(())
            }
            (PipelineState::Qualified, PipelineState::ProposalSent) => {
                self.current_state = next_state;
                Ok(())
            }
            (PipelineState::ProposalSent, PipelineState::Negotiation) => {
                self.current_state = next_state;
                Ok(())
            }
            (PipelineState::Negotiation, PipelineState::ClosedWon) => {
                self.current_state = next_state;
                Ok(())
            }
            (PipelineState::Negotiation, PipelineState::ClosedLost) => {
                self.current_state = next_state;
                Ok(())
            }
            (_, PipelineState::ClosedLost) => {
                self.current_state = next_state;
                Ok(())
            }
            _ => Err("Invalid pipeline transition"),
        }
    }
}
