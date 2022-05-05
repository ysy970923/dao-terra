use cosmwasm_std::{Deps, StdResult, Uint128};

use crate::error::ContractError;
use crate::state::{
    bank_read, config_read, poll_read, read_poll_voters, read_polls, state_read, Config, Poll,
    State,
};
use crate::utils::{
    ConfigResponse, OrderBy, PollResponse, PollStatus, PollsResponse, StakerResponse,
    StateResponse, VotersResponse, VotersResponseItem,
};

/// query configurations
pub fn query_config(deps: Deps) -> Result<ConfigResponse, ContractError> {
    let config: Config = config_read(deps.storage).load()?;
    Ok(ConfigResponse {
        owner: deps.api.addr_humanize(&config.owner)?.to_string(),
        quorum: config.quorum,
        threshold: config.threshold,
        voting_period: config.voting_period,
    })
}

/// query current state
pub fn query_state(deps: Deps) -> Result<StateResponse, ContractError> {
    let state: State = state_read(deps.storage).load()?;
    Ok(StateResponse {
        poll_count: state.poll_count,
        total_share: state.total_share,
    })
}

/// query single poll
pub fn query_poll(deps: Deps, poll_id: u64) -> Result<PollResponse, ContractError> {
    let poll = match poll_read(deps.storage).may_load(&poll_id.to_be_bytes())? {
        Some(poll) => Some(poll),
        None => return Err(ContractError::PollNotFound {}),
    }
    .unwrap();

    Ok(PollResponse {
        id: poll.id,
        creator: deps.api.addr_humanize(&poll.creator)?.to_string(),
        status: poll.status,
        end_height: poll.end_height,
        title: poll.title,
        description: poll.description,
        link: poll.link,
        yes_votes: poll.yes_votes,
        no_votes: poll.no_votes,
        total_share_at_start_poll: poll.total_share_at_start_poll,
        total_share_at_end_poll: poll.total_share_at_end_poll,
    })
}

/// query multiple polls between time period
pub fn query_polls(
    deps: Deps,
    filter: Option<PollStatus>,
    start_after: Option<u64>,
    limit: Option<u32>,
    order_by: Option<OrderBy>,
) -> Result<PollsResponse, ContractError> {
    let polls = read_polls(deps.storage, filter, start_after, limit, order_by)?;

    let poll_responses: StdResult<Vec<PollResponse>> = polls
        .iter()
        .map(|poll| {
            Ok(PollResponse {
                id: poll.id,
                creator: deps.api.addr_humanize(&poll.creator)?.to_string(),
                status: poll.status.clone(),
                end_height: poll.end_height,
                title: poll.title.to_string(),
                description: poll.description.to_string(),
                link: poll.link.clone(),
                yes_votes: poll.yes_votes,
                no_votes: poll.no_votes,
                total_share_at_start_poll: poll.total_share_at_start_poll,
                total_share_at_end_poll: poll.total_share_at_end_poll,
            })
        })
        .collect();

    Ok(PollsResponse {
        polls: poll_responses?,
    })
}

pub fn query_voters(
    deps: Deps,
    poll_id: u64,
    start_after: Option<String>,
    limit: Option<u32>,
    order_by: Option<OrderBy>,
) -> Result<VotersResponse, ContractError> {
    let poll: Poll = match poll_read(deps.storage).may_load(&poll_id.to_be_bytes())? {
        Some(poll) => Some(poll),
        None => return Err(ContractError::PollNotFound {}),
    }
    .unwrap();

    let voters = if poll.status != PollStatus::InProgress {
        vec![]
    } else if let Some(start_after) = start_after {
        read_poll_voters(
            deps.storage,
            poll_id,
            Some(deps.api.addr_canonicalize(&start_after)?),
            limit,
            order_by,
        )?
    } else {
        read_poll_voters(deps.storage, poll_id, None, limit, order_by)?
    };

    let voters_response: StdResult<Vec<VotersResponseItem>> = voters
        .iter()
        .map(|voter_info| {
            Ok(VotersResponseItem {
                voter: deps.api.addr_humanize(&voter_info.0)?.to_string(),
                vote: voter_info.1.vote.clone(),
                balance: voter_info.1.balance,
            })
        })
        .collect();

    Ok(VotersResponse {
        voters: voters_response?,
    })
}

pub fn query_staker(deps: Deps, address: String) -> StdResult<StakerResponse> {
    let addr_raw = deps.api.addr_canonicalize(&address).unwrap();
    let state: State = state_read(deps.storage).load()?;
    let mut token_manager = bank_read(deps.storage)
        .may_load(addr_raw.as_slice())?
        .unwrap_or_default();

    // leave only in-progress polls
    token_manager.locked_share.retain(|(poll_id, _)| {
        let poll: Poll = poll_read(deps.storage)
            .load(&poll_id.to_be_bytes())
            .unwrap();

        poll.status == PollStatus::InProgress
    });

    Ok(StakerResponse {
        balance: if !state.total_share.is_zero() {
            token_manager.share
        } else {
            Uint128::zero()
        },
        share: token_manager.share,
        locked_balance: token_manager.locked_share,
    })
}
