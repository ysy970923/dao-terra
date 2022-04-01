use cosmwasm_std::{Binary, Decimal, Uint128};
use cw20::Cw20ReceiveMsg;
use crate::utils::{PollStatus, VoteOption, OrderBy};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/**
{
    "cw20_token": "terra1lzfrsy38l34uzrlma3fm3hsktv848pcgdnlcj7",
    "quorum": "0.1",
    "threshold": "0.5",
    "voting_period": 50,
    "timelock_period": 50,
    "proposal_deposit": "10",
    "snapshot_period": 30
}
**/
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InstantiateMsg {
    pub cw20_token: String,
    pub quorum: Decimal,
    pub threshold: Decimal,
    pub voting_period: u64,
    pub timelock_period: u64,
    pub proposal_deposit: Uint128,
    pub snapshot_period: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    Receive(Cw20ReceiveMsg),
    ExecutePollMsgs {
        poll_id: u64,
    },
    UpdateConfig {
        owner: Option<String>,
        cw20_token: Option<String>,
        quorum: Option<Decimal>,
        threshold: Option<Decimal>,
        voting_period: Option<u64>,
        timelock_period: Option<u64>,
        proposal_deposit: Option<Uint128>,
        snapshot_period: Option<u64>,
    },
    /*
    {"cast_vote": {
        "poll_id": 2,
        "vote": "yes",
        "amount": "1"
        }
    } 
    */
    CastVote {
        poll_id: u64,
        vote: VoteOption,
        amount: Uint128,
    },
    WithdrawVotingTokens {
        amount: Option<Uint128>,
    },
    EndPoll {
        poll_id: u64,
    },
    ExecutePoll {
        poll_id: u64,
    },
    SnapshotPoll {
        poll_id: u64,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    Config {},
    State {},
    Staker {
        address: String,
    },
    Poll {
        poll_id: u64,
    },
    /*
    {"polls": {
        "filter": "in_progress"
        }
    }
    */
    Polls {
        filter: Option<PollStatus>,
        start_after: Option<u64>,
        limit: Option<u32>,
        order_by: Option<OrderBy>,
    },
    Voters {
        poll_id: u64,
        start_after: Option<String>,
        limit: Option<u32>,
        order_by: Option<OrderBy>,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Cw20HookMsg {
    StakeVotingTokens {},
    CreatePoll {
        title: String,
        description: String,
        link: Option<String>,
        execute_msgs: Option<Vec<PollExecuteMsg>>,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct PollExecuteMsg {
    pub order: u64,
    pub contract: String,
    pub msg: Binary,
}
