#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_binary, Binary, Decimal, Deps, DepsMut, Env, MessageInfo, Response, StdError, StdResult,
    Uint128,
};
use cw2::set_contract_version;

use crate::error::ContractError;
use crate::msg::{ExecuteMsg, InstantiateMsg, QueryMsg};
use crate::state::{config_store, state_store, Config, State};

use crate::execute::{
    cancel_vote, cast_vote, create_poll, delegate_vote, end_poll, instant_burn, mint,
    transfer_from, undelegate_vote, update_config,
};

use crate::query::{
    query_config, query_poll, query_polls, query_staker, query_state, query_voters,
};

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:dao-gov";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    validate_quorum(msg.quorum)?;
    validate_threshold(msg.threshold)?;

    let config = Config {
        owner: deps.api.addr_canonicalize(info.sender.as_str())?,
        cw721_token: deps.api.addr_canonicalize(&msg.cw721_token)?,
        quorum: msg.quorum,
        threshold: msg.threshold,
        voting_period: msg.voting_period,
    };

    let state = State {
        contract_addr: deps.api.addr_canonicalize(env.contract.address.as_str())?,
        poll_count: 0,
        total_share: Uint128::zero(),
    };

    config_store(deps.storage).save(&config)?;
    state_store(deps.storage).save(&state)?;

    Ok(Response::default())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::Mint { recipient, amount } => mint(deps, info, recipient, amount),
        ExecuteMsg::InstantBurn { amount } => instant_burn(deps, info, amount),
        ExecuteMsg::TransferFrom {
            owner,
            recipient,
            amount,
        } => transfer_from(deps, info, owner, recipient, amount),
        ExecuteMsg::DelegateVote { delegator } => delegate_vote(deps, info, delegator),
        ExecuteMsg::UnDelegateVote {} => undelegate_vote(deps, info),
        ExecuteMsg::CreatePoll {
            title,
            description,
            link,
        } => create_poll(deps, env, info, title, description, link),
        ExecuteMsg::UpdateConfig {
            owner,
            quorum,
            threshold,
            voting_period,
        } => update_config(deps, info, owner, quorum, threshold, voting_period),
        ExecuteMsg::CastVote { poll_id, vote } => cast_vote(deps, env, info, poll_id, vote),
        ExecuteMsg::CancelVote { poll_id } => cancel_vote(deps, env, info, poll_id),
        ExecuteMsg::EndPoll { poll_id } => end_poll(deps, env, poll_id),
    }
}

// quorum: 정족수
// 0~1
fn validate_quorum(quorum: Decimal) -> StdResult<()> {
    if quorum > Decimal::one() {
        Err(StdError::generic_err("quorum must be 0 to 1"))
    } else {
        Ok(())
    }
}

// 0~1
fn validate_threshold(threshold: Decimal) -> StdResult<()> {
    if threshold > Decimal::one() {
        Err(StdError::generic_err("threshold must be 0 to 1"))
    } else {
        Ok(())
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> Result<Binary, ContractError> {
    match msg {
        QueryMsg::Config {} => Ok(to_binary(&query_config(deps)?)?),
        QueryMsg::State {} => Ok(to_binary(&query_state(deps)?)?),
        QueryMsg::Staker { address } => Ok(to_binary(&query_staker(deps, address)?)?),
        QueryMsg::Poll { poll_id } => Ok(to_binary(&query_poll(deps, poll_id)?)?),
        QueryMsg::Polls {
            filter,
            start_after,
            limit,
            order_by,
        } => Ok(to_binary(&query_polls(
            deps,
            filter,
            start_after,
            limit,
            order_by,
        )?)?),
        QueryMsg::Voters {
            poll_id,
            start_after,
            limit,
            order_by,
        } => Ok(to_binary(&query_voters(
            deps,
            poll_id,
            start_after,
            limit,
            order_by,
        )?)?),
    }
}
