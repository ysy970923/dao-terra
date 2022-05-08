use crate::utils::{OrderBy, PollStatus, VoteOption};
use cosmwasm_std::{Binary, Decimal, Uint128};
use cw721::Cw721ReceiveMsg;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/**
{
    "cw721_token": "terra1c929yvrhcng9l93lska29hajewlvml4frdpyre",
    "quorum": "0.1",
    "threshold": "0.5",
    "voting_period": 100
}
**/
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InstantiateMsg {
    pub cw721_token: String,
    pub quorum: Decimal,
    pub threshold: Decimal,
    pub voting_period: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    ReceiveNft(Cw721ReceiveMsg),
    /**
    {
        "mint": {
            "recipient": "1",
            "amount": "100000000"
        }
    }
     */
    Mint {
        recipient: String,
        amount: Uint128,
    },
    TransferFrom {
        owner: String,
        recipient: String,
        amount: Uint128,
    },
    UpdateConfig {
        owner: Option<String>,
        quorum: Option<Decimal>,
        threshold: Option<Decimal>,
        voting_period: Option<u64>,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Cw721HookMsg {
    Exit {},
    DelegateVote {
        delegator: String,
    },
    UnDelegateVote {},
    CreatePoll {
        title: String,
        description: String,
        link: Option<String>,
    },
    /*
    {"cast_vote": {
        "poll_id": 2,
        "vote": "yes",
        "amount": "100000"
        }
    }
    */
    CastVote {
        poll_id: u64,
        vote: VoteOption,
    },
    CancelVote {
        poll_id: u64,
    },
    /*
    {"end_poll": {
        "poll_id": 2
        }
    }
    */
    EndPoll {
        poll_id: u64,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    Config {},
    State {},
    Member {
        member_id: String,
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
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct PollExecuteMsg {
    pub order: u64,
    pub contract: String,
    pub msg: Binary,
}
