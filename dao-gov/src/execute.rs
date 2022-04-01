use cosmwasm_std::{
    attr, from_binary, to_binary, Addr, CanonicalAddr, CosmosMsg, Decimal, DepsMut, Env,
    MessageInfo, Response, Storage, SubMsg, Uint128, WasmMsg,
};
use cw20::{Cw20ExecuteMsg, Cw20ReceiveMsg};

use crate::error::ContractError;
use crate::msg::{Cw20HookMsg, ExecuteMsg, PollExecuteMsg};
use crate::state::{
    bank_read, bank_store, config_read, config_store, poll_indexer_store, poll_read, poll_store,
    poll_voter_read, poll_voter_store, state_read, state_store, store_tmp_poll_id, Config,
    ExecuteData, Poll, State, TokenManager,
};
use crate::utils::{
    query_token_balance, validate_description, validate_link, validate_title, PollStatus, VoteInfo,
    VoteOption,
};

pub fn receive_cw20(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    cw20_msg: Cw20ReceiveMsg,
) -> Result<Response, ContractError> {
    let config: Config = config_read(deps.storage).load()?;
    // cw 20 contract only has authorization
    if config.cw20_token != deps.api.addr_canonicalize(info.sender.as_str())? {
        return Err(ContractError::Unauthorized {});
    }

    match from_binary(&cw20_msg.msg) {
        Ok(Cw20HookMsg::StakeVotingTokens {}) => {
            let api = deps.api;
            stake_voting_tokens(deps, api.addr_validate(&cw20_msg.sender)?, cw20_msg.amount)
        }
        Ok(Cw20HookMsg::CreatePoll {
            title,
            description,
            link,
            execute_msgs,
        }) => create_poll(
            deps,
            env,
            cw20_msg.sender,
            cw20_msg.amount,
            title,
            description,
            link,
            execute_msgs,
        ),
        _ => Err(ContractError::DataShouldBeGiven {}),
    }
}

fn create_poll(
    deps: DepsMut,
    env: Env,
    proposer: String,
    deposit_amount: Uint128,
    title: String,
    description: String,
    link: Option<String>,
    execute_msgs: Option<Vec<PollExecuteMsg>>,
) -> Result<Response, ContractError> {
    validate_title(&title)?;
    validate_description(&description)?;
    validate_link(&link)?;

    let config: Config = config_store(deps.storage).load()?;
    if deposit_amount < config.proposal_deposit {
        return Err(ContractError::InsufficientProposalDeposit(
            config.proposal_deposit.u128(),
        ));
    }

    let mut state: State = state_store(deps.storage).load()?;
    let poll_id = state.poll_count + 1;

    // Increase poll count & total deposit amount
    state.poll_count += 1;
    state.total_deposit += deposit_amount;

    let mut data_list: Vec<ExecuteData> = vec![];
    let all_execute_data = if let Some(exe_msgs) = execute_msgs {
        for msgs in exe_msgs {
            let execute_data = ExecuteData {
                order: msgs.order,
                contract: deps.api.addr_canonicalize(&msgs.contract)?,
                msg: msgs.msg,
            };
            data_list.push(execute_data)
        }
        Some(data_list)
    } else {
        None
    };

    let sender_address_raw = deps.api.addr_canonicalize(&proposer)?;
    let new_poll = Poll {
        id: poll_id,
        creator: sender_address_raw,
        status: PollStatus::InProgress,
        yes_votes: Uint128::zero(),
        no_votes: Uint128::zero(),
        end_height: env.block.height + config.voting_period,
        title,
        description,
        link,
        execute_data: all_execute_data,
        deposit_amount,
        total_balance_at_end_poll: None,
        staked_amount: None,
    };

    poll_store(deps.storage).save(&poll_id.to_be_bytes(), &new_poll)?;
    poll_indexer_store(deps.storage, &PollStatus::InProgress)
        .save(&poll_id.to_be_bytes(), &true)?;

    state_store(deps.storage).save(&state)?;

    Ok(Response::new().add_attributes(vec![
        ("action", "create_poll"),
        (
            "creator",
            deps.api
                .addr_humanize(&new_poll.creator)?
                .to_string()
                .as_str(),
        ),
        ("poll_id", &poll_id.to_string()),
        ("end_height", new_poll.end_height.to_string().as_str()),
    ]))
}

const POLL_EXECUTE_REPLY_ID: u64 = 1;
/*
 * Execute a msgs of passed poll as one submsg to catch failures
 */
pub fn execute_poll(deps: DepsMut, env: Env, poll_id: u64) -> Result<Response, ContractError> {
    let config: Config = config_read(deps.storage).load()?;
    let a_poll: Poll = poll_store(deps.storage).load(&poll_id.to_be_bytes())?;

    if a_poll.status != PollStatus::Passed {
        return Err(ContractError::PollNotPassed {});
    }

    if a_poll.end_height + config.timelock_period > env.block.height {
        return Err(ContractError::TimelockNotExpired {});
    }

    store_tmp_poll_id(deps.storage, a_poll.id)?;

    Ok(Response::new().add_submessage(SubMsg::reply_on_error(
        CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: env.contract.address.to_string(),
            msg: to_binary(&ExecuteMsg::ExecutePollMsgs { poll_id })?,
            funds: vec![],
        }),
        POLL_EXECUTE_REPLY_ID,
    )))
}

/// only called by this contract
pub fn execute_poll_messages(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    poll_id: u64,
) -> Result<Response, ContractError> {
    if env.contract.address != info.sender {
        return Err(ContractError::Unauthorized {});
    }

    let mut a_poll: Poll = poll_store(deps.storage).load(&poll_id.to_be_bytes())?;
    // poll_id: remove from passed polls, add to executed polls
    poll_indexer_store(deps.storage, &PollStatus::Passed).remove(&poll_id.to_be_bytes());
    poll_indexer_store(deps.storage, &PollStatus::Executed).save(&poll_id.to_be_bytes(), &true)?;

    // change poll status and save
    a_poll.status = PollStatus::Executed;
    poll_store(deps.storage).save(&poll_id.to_be_bytes(), &a_poll)?;

    // execute the msgs in the poll in order
    let mut messages: Vec<CosmosMsg> = vec![];
    if let Some(all_msgs) = a_poll.execute_data {
        let mut msgs = all_msgs;
        msgs.sort();
        for msg in msgs {
            messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: deps.api.addr_humanize(&msg.contract)?.to_string(),
                msg: msg.msg,
                funds: vec![],
            }));
        }
    }

    Ok(Response::new().add_messages(messages).add_attributes(vec![
        ("action", "execute_poll"),
        ("poll_id", poll_id.to_string().as_str()),
    ]))
}

/// SnapshotPoll is used to take a snapshot of the staked amount for quorum calculation
pub fn snapshot_poll(deps: DepsMut, env: Env, poll_id: u64) -> Result<Response, ContractError> {
    let config: Config = config_read(deps.storage).load()?;
    let mut a_poll: Poll = poll_store(deps.storage).load(&poll_id.to_be_bytes())?;

    if a_poll.status != PollStatus::InProgress {
        return Err(ContractError::PollNotInProgress {});
    }

    let time_to_end = a_poll.end_height - env.block.height;

    // too much time left
    if time_to_end > config.snapshot_period {
        return Err(ContractError::SnapshotHeight {});
    }

    if a_poll.staked_amount.is_some() {
        return Err(ContractError::SnapshotAlreadyOccurred {});
    }

    // store the current staked amount for quorum calculation
    let state: State = state_store(deps.storage).load()?;

    let staked_amount = query_token_balance(
        &deps.querier,
        deps.api.addr_humanize(&config.cw20_token)?,
        deps.api.addr_humanize(&state.contract_addr)?,
    )?
    .checked_sub(state.total_deposit)?;

    a_poll.staked_amount = Some(staked_amount);

    poll_store(deps.storage).save(&poll_id.to_be_bytes(), &a_poll)?;

    Ok(Response::new().add_attributes(vec![
        attr("action", "snapshot_poll"),
        attr("poll_id", poll_id.to_string().as_str()),
        attr("staked_amount", staked_amount.to_string().as_str()),
    ]))
}

/// cast vote
pub fn cast_vote(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    poll_id: u64,
    vote: VoteOption,
    amount: Uint128,
) -> Result<Response, ContractError> {
    let sender_address_raw = deps.api.addr_canonicalize(info.sender.as_str())?;
    let config = config_read(deps.storage).load()?;
    let state = state_read(deps.storage).load()?;

    // check if valid poll id
    if poll_id == 0 || state.poll_count < poll_id {
        return Err(ContractError::PollNotFound {});
    }

    // check if poll is in progress and not ended
    let mut a_poll: Poll = poll_store(deps.storage).load(&poll_id.to_be_bytes())?;
    if a_poll.status != PollStatus::InProgress || env.block.height > a_poll.end_height {
        return Err(ContractError::PollNotInProgress {});
    }

    // check if sender_address already voted
    if poll_voter_read(deps.storage, poll_id)
        .load(sender_address_raw.as_slice())
        .is_ok()
    {
        return Err(ContractError::AlreadyVoted {});
    }

    let key = &sender_address_raw.as_slice();
    let mut token_manager = bank_read(deps.storage).may_load(key)?.unwrap_or_default();

    let total_share = state.total_share;
    let total_balance = query_token_balance(
        &deps.querier,
        deps.api.addr_humanize(&config.cw20_token)?,
        deps.api.addr_humanize(&state.contract_addr)?,
    )?
    .checked_sub(state.total_deposit)?;

    // check if more staked than casted amount
    if token_manager
        .share
        .multiply_ratio(total_balance, total_share)
        < amount
    {
        return Err(ContractError::InsufficientFunds {});
    }

    // increment yes/no votes
    if vote == VoteOption::Yes {
        a_poll.yes_votes += amount;
    } else {
        a_poll.no_votes += amount;
    }

    // save vote info to voter's token manager
    let vote_info = VoteInfo {
        vote,
        balance: amount,
    };
    token_manager
        .locked_balance
        .push((poll_id, vote_info.clone()));
    bank_store(deps.storage).save(key, &token_manager)?;

    // store poll voter, update poll data
    poll_voter_store(deps.storage, poll_id).save(sender_address_raw.as_slice(), &vote_info)?;

    // processing snapshot
    let time_to_end = a_poll.end_height - env.block.height;

    if time_to_end < config.snapshot_period && a_poll.staked_amount.is_none() {
        a_poll.staked_amount = Some(total_balance);
    }

    poll_store(deps.storage).save(&poll_id.to_be_bytes(), &a_poll)?;

    Ok(Response::new().add_attributes(vec![
        ("action", "cast_vote"),
        ("poll_id", poll_id.to_string().as_str()),
        ("amount", amount.to_string().as_str()),
        ("voter", info.sender.as_str()),
        ("vote_option", vote_info.vote.to_string().as_str()),
    ]))
}

/// ends poll
pub fn end_poll(deps: DepsMut, env: Env, poll_id: u64) -> Result<Response, ContractError> {
    let mut a_poll: Poll = poll_store(deps.storage).load(&poll_id.to_be_bytes())?;

    if a_poll.status != PollStatus::InProgress {
        return Err(ContractError::PollNotInProgress {});
    }

    if a_poll.end_height > env.block.height {
        return Err(ContractError::PollVotingPeriod {});
    }

    let no = a_poll.no_votes.u128();
    let yes = a_poll.yes_votes.u128();

    let tallied_weight = yes + no;

    let mut poll_status = PollStatus::Rejected;
    let mut rejected_reason = "";
    let mut passed = false;

    let mut messages: Vec<CosmosMsg> = vec![];
    let config: Config = config_read(deps.storage).load()?;
    let mut state: State = state_read(deps.storage).load()?;

    // if total_share is 0
    let (quorum, staked_weight) = if state.total_share.u128() == 0 {
        (Decimal::zero(), Uint128::zero())
    } else if let Some(staked_amount) = a_poll.staked_amount {
        // snapshoted
        (
            Decimal::from_ratio(tallied_weight, staked_amount),
            staked_amount,
        )
    } else {
        // not snapshoted
        let staked_weight = query_token_balance(
            &deps.querier,
            deps.api.addr_humanize(&config.cw20_token)?,
            deps.api.addr_humanize(&state.contract_addr)?,
        )?
        .checked_sub(state.total_deposit)?;

        (
            Decimal::from_ratio(tallied_weight, staked_weight),
            staked_weight,
        )
    };

    if tallied_weight == 0 || quorum < config.quorum {
        rejected_reason = "Quorum not reached";
    } else {
        // poll passed
        if Decimal::from_ratio(yes, tallied_weight) > config.threshold {
            poll_status = PollStatus::Passed;
            passed = true;
        } else {
            rejected_reason = "Threshold not reached";
        }

        // refund, if deposit is not 0, transfer back to recipient
        if !a_poll.deposit_amount.is_zero() {
            messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: deps.api.addr_humanize(&config.cw20_token)?.to_string(),
                funds: vec![],
                msg: to_binary(&Cw20ExecuteMsg::Transfer {
                    recipient: deps.api.addr_humanize(&a_poll.creator)?.to_string(),
                    amount: a_poll.deposit_amount,
                })?,
            }))
        }
    }

    // Decrease total deposit amount
    state.total_deposit = state.total_deposit.checked_sub(a_poll.deposit_amount)?;
    state_store(deps.storage).save(&state)?;

    // Update poll indexer, remove from in progress and add to new poll status indexer
    poll_indexer_store(deps.storage, &PollStatus::InProgress).remove(&a_poll.id.to_be_bytes());
    poll_indexer_store(deps.storage, &poll_status).save(&a_poll.id.to_be_bytes(), &true)?;

    // Update poll status
    a_poll.status = poll_status;
    a_poll.total_balance_at_end_poll = Some(staked_weight);
    poll_store(deps.storage).save(&poll_id.to_be_bytes(), &a_poll)?;

    Ok(Response::new().add_messages(messages).add_attributes(vec![
        ("action", "end_poll"),
        ("poll_id", &poll_id.to_string()),
        ("rejected_reason", rejected_reason),
        ("passed", &passed.to_string()),
    ]))
}

#[allow(clippy::too_many_arguments)]
pub fn update_config(
    deps: DepsMut,
    info: MessageInfo,
    owner: Option<String>,
    cw20_token: Option<String>,
    quorum: Option<Decimal>,
    threshold: Option<Decimal>,
    voting_period: Option<u64>,
    timelock_period: Option<u64>,
    proposal_deposit: Option<Uint128>,
    snapshot_period: Option<u64>,
) -> Result<Response, ContractError> {
    let api = deps.api;
    config_store(deps.storage).update(|mut config| {
        if config.owner != api.addr_canonicalize(info.sender.as_str())? {
            return Err(ContractError::Unauthorized {});
        }
        if let Some(owner) = owner {
            config.owner = api.addr_canonicalize(&owner)?;
        }
        if let Some(cw20_token) = cw20_token {
            config.cw20_token = api.addr_canonicalize(&cw20_token)?;
        }
        if let Some(quorum) = quorum {
            config.quorum = quorum;
        }
        if let Some(threshold) = threshold {
            config.threshold = threshold;
        }
        if let Some(voting_period) = voting_period {
            config.voting_period = voting_period;
        }
        if let Some(timelock_period) = timelock_period {
            config.timelock_period = timelock_period;
        }
        if let Some(proposal_deposit) = proposal_deposit {
            config.proposal_deposit = proposal_deposit;
        }
        if let Some(period) = snapshot_period {
            config.snapshot_period = period;
        }
        Ok(config)
    })?;

    Ok(Response::new().add_attributes(vec![("action", "update_config")]))
}

/// stake voting tokens
pub fn stake_voting_tokens(
    deps: DepsMut,
    sender: Addr,
    amount: Uint128,
) -> Result<Response, ContractError> {
    if amount.is_zero() {
        return Err(ContractError::InsufficientFunds {});
    }

    let sender_address_raw = deps.api.addr_canonicalize(sender.as_str())?;
    let key = &sender_address_raw.as_slice();

    let mut token_manager = bank_read(deps.storage).may_load(key)?.unwrap_or_default();
    let config: Config = config_store(deps.storage).load()?;
    let mut state: State = state_store(deps.storage).load()?;

    // total_balance = token_balance - proposal_deposit - amount(alreay transferred)
    let total_balance = query_token_balance(
        &deps.querier,
        deps.api.addr_humanize(&config.cw20_token)?,
        deps.api.addr_humanize(&state.contract_addr)?,
    )?
    .checked_sub(state.total_deposit + amount)?;

    // share = amount / total_staked_tokens
    let share = if total_balance.is_zero() || state.total_share.is_zero() {
        amount
    } else {
        amount.multiply_ratio(state.total_share, total_balance)
    };

    token_manager.share += share;
    state.total_share += share;

    state_store(deps.storage).save(&state)?;
    bank_store(deps.storage).save(key, &token_manager)?;

    Ok(Response::new().add_attributes(vec![
        ("action", "staking"),
        ("sender", sender.as_str()),
        ("share", share.to_string().as_str()),
        ("amount", amount.to_string().as_str()),
    ]))
}

/// return maximum balance between in progress polls
fn compute_locked_balance(
    storage: &mut dyn Storage,
    token_manager: &mut TokenManager,
    voter: &CanonicalAddr,
) -> u128 {
    // only leave in progress polls
    token_manager.locked_balance.retain(|(poll_id, _)| {
        let poll: Poll = poll_read(storage).load(&poll_id.to_be_bytes()).unwrap();

        // remove voter if poll is not in progress
        if poll.status != PollStatus::InProgress {
            poll_voter_store(storage, *poll_id).remove(voter.as_slice());
        }

        poll.status == PollStatus::InProgress
    });

    token_manager
        .locked_balance
        .iter()
        .map(|(_, v)| v.balance.u128())
        .max()
        .unwrap_or_default()
}

/// withdraw staked tokens
pub fn withdraw_voting_tokens(
    deps: DepsMut,
    info: MessageInfo,
    amount: Option<Uint128>,
) -> Result<Response, ContractError> {
    let sender_address_raw = deps.api.addr_canonicalize(info.sender.as_str())?;
    let key = sender_address_raw.as_slice();

    if let Some(mut token_manager) = bank_read(deps.storage).may_load(key)? {
        let config: Config = config_store(deps.storage).load()?;
        let mut state: State = state_store(deps.storage).load()?;

        // Load total share & total balance except proposal deposit amount
        let total_share = state.total_share.u128();
        let total_balance = query_token_balance(
            &deps.querier,
            deps.api.addr_humanize(&config.cw20_token)?,
            deps.api.addr_humanize(&state.contract_addr)?,
        )?
        .checked_sub(state.total_deposit)?
        .u128();

        let locked_balance =
            compute_locked_balance(deps.storage, &mut token_manager, &sender_address_raw);
        let locked_share = locked_balance * total_share / total_balance;
        let user_share = token_manager.share.u128();

        let withdraw_share = amount
            .map(|v| std::cmp::max(v.multiply_ratio(total_share, total_balance).u128(), 1u128))
            .unwrap_or_else(|| user_share - locked_share);
        let withdraw_amount = amount
            .map(|v| v.u128())
            .unwrap_or_else(|| withdraw_share * total_balance / total_share);

        if locked_share + withdraw_share > user_share {
            Err(ContractError::InvalidWithdrawAmount {})
        } else {
            let share = user_share - withdraw_share;
            token_manager.share = Uint128::from(share);

            bank_store(deps.storage).save(key, &token_manager)?;

            state.total_share = Uint128::from(total_share - withdraw_share);
            state_store(deps.storage).save(&state)?;

            let contract_human = deps.api.addr_humanize(&config.cw20_token)?.to_string();
            let recipient_human = deps.api.addr_humanize(&sender_address_raw)?.to_string();
            Ok(Response::new()
                .add_messages(vec![CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: contract_human,
                    msg: to_binary(&Cw20ExecuteMsg::Transfer {
                        recipient: recipient_human.clone(),
                        amount: Uint128::from(withdraw_amount),
                    })?,
                    funds: vec![],
                })])
                .add_attributes(vec![
                    ("action", "withdraw"),
                    ("recipient", recipient_human.as_str()),
                    ("amount", withdraw_amount.to_string().as_str()),
                ]))
        }
    } else {
        Err(ContractError::NothingStaked {})
    }
}
