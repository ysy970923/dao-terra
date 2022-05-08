use cosmwasm_std::{
    from_binary, Decimal, DepsMut, Env, Isqrt, MessageInfo, Response, Storage, Uint128,
};

use crate::error::ContractError;
use crate::msg::Cw721HookMsg;
use crate::state::{
    bank_read, bank_store, config_read, config_store, poll_indexer_store, poll_read, poll_store,
    poll_voter_read, poll_voter_store, state_read, state_store, Config, Poll, State, TokenManager,
};
use crate::utils::{
    validate_description, validate_link, validate_title, PollStatus, VoteInfo, VoteOption,
};
use cw721::Cw721ReceiveMsg;
pub fn receive_cw721(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    cw721_msg: Cw721ReceiveMsg,
) -> Result<Response, ContractError> {
    let config: Config = config_read(deps.storage).load()?;

    if config.cw721_token != deps.api.addr_canonicalize(info.sender.as_str())? {
        return Err(ContractError::Unauthorized {});
    }
    match from_binary(&cw721_msg.msg) {
        Ok(Cw721HookMsg::CastVote { poll_id, vote }) => {
            cast_vote(deps, env, cw721_msg.token_id, poll_id, vote)
        }
        Ok(Cw721HookMsg::CancelVote { poll_id }) => {
            cancel_vote(deps, env, cw721_msg.token_id, poll_id)
        }
        Ok(Cw721HookMsg::CreatePoll {
            title,
            description,
            link,
        }) => create_poll(deps, env, cw721_msg.token_id, title, description, link),
        Ok(Cw721HookMsg::EndPoll { poll_id }) => end_poll(deps, env, poll_id),
        Ok(Cw721HookMsg::DelegateVote { delegator }) => {
            delegate_vote(deps, cw721_msg.token_id, delegator)
        }
        Ok(Cw721HookMsg::UnDelegateVote {}) => undelegate_vote(deps, cw721_msg.token_id),
        Ok(Cw721HookMsg::Exit {}) => exit(deps, cw721_msg.token_id),
        _ => Err(ContractError::DataShouldBeGiven {}),
    }
}

fn create_poll(
    deps: DepsMut,
    env: Env,
    sender_id: String,
    title: String,
    description: String,
    link: Option<String>,
) -> Result<Response, ContractError> {
    validate_title(&title)?;
    validate_description(&description)?;
    validate_link(&link)?;
    let config: Config = config_store(deps.storage).load()?;

    let mut state: State = state_store(deps.storage).load()?;
    let poll_id = state.poll_count + 1;

    // Increase poll count & total deposit amount
    state.poll_count += 1;

    let new_poll = Poll {
        id: poll_id,
        creator: sender_id.clone(),
        status: PollStatus::InProgress,
        yes_votes: Uint128::zero(),
        no_votes: Uint128::zero(),
        end_height: env.block.height + config.voting_period,
        title,
        description,
        link,
        total_share_at_start_poll: state.total_share,
        total_share_at_end_poll: None,
    };

    poll_store(deps.storage).save(&poll_id.to_be_bytes(), &new_poll)?;
    poll_indexer_store(deps.storage, &PollStatus::InProgress)
        .save(&poll_id.to_be_bytes(), &true)?;

    state_store(deps.storage).save(&state)?;

    Ok(Response::new().add_attributes(vec![
        ("action", "create_poll"),
        ("creator", sender_id.as_str()),
        ("poll_id", &poll_id.to_string()),
        ("end_height", new_poll.end_height.to_string().as_str()),
    ]))
}

/// cast vote (can't vote if delegated)
fn cast_vote(
    deps: DepsMut,
    env: Env,
    voter_id: String,
    poll_id: u64,
    vote: VoteOption,
) -> Result<Response, ContractError> {
    let voter_key = voter_id.as_bytes();
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

    // check if already voted
    if poll_voter_read(deps.storage, poll_id)
        .load(voter_key)
        .is_ok()
    {
        return Err(ContractError::AlreadyVoted {});
    }

    let token_manager = bank_read(deps.storage)
        .may_load(voter_key)?
        .unwrap_or_default();

    // delegated user can't cast vote (must undelegate first)
    if token_manager.delegate_to.is_some() {
        return Err(ContractError::AlreadyDelegated {});
    }

    // cast my vote
    let mut total_amount = cast_single_vote(deps.storage, voter_key, &mut a_poll, vote.clone())?;

    // cast delegated votes
    for id in token_manager.delegated_from.iter() {
        let amount = cast_single_vote(deps.storage, id.as_bytes(), &mut a_poll, vote.clone())?;
        total_amount += amount;
    }

    poll_store(deps.storage).save(&poll_id.to_be_bytes(), &a_poll)?;

    Ok(Response::new().add_attributes(vec![
        ("action", "cast_vote"),
        ("poll_id", poll_id.to_string().as_str()),
        ("my_share", token_manager.share.to_string().as_str()),
        ("total_amount", total_amount.to_string().as_str()),
        ("voter", voter_id.as_str()),
        ("vote_option", vote.to_string().as_str()),
    ]))
}

/// cast single vote used in cast vote
/// not check already voted (not voted -> delegated, delegated -> not voted)
/// delegated from member can't be voted
fn cast_single_vote(
    storage: &mut dyn Storage,
    voter_key: &[u8],
    a_poll: &mut Poll,
    vote: VoteOption,
) -> Result<u128, ContractError> {
    let poll_id = a_poll.id;
    let mut token_manager = bank_read(storage).may_load(voter_key)?.unwrap_or_default();

    let amount = token_manager.share;
    // if amount.is_zero() {
    //     return Ok(0);
    // }

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
        .locked_share
        .push((poll_id, vote_info.clone()));
    bank_store(storage).save(voter_key, &token_manager)?;

    // store poll voter, update poll data
    poll_voter_store(storage, poll_id).save(voter_key, &vote_info)?;

    Ok(amount.u128())
}

/// ends poll
fn end_poll(deps: DepsMut, env: Env, poll_id: u64) -> Result<Response, ContractError> {
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

    let config: Config = config_read(deps.storage).load()?;
    let state: State = state_read(deps.storage).load()?;
    let total_share = state.total_share;

    // if total_share is 0
    let quorum = if state.total_share.u128() == 0 {
        Decimal::zero()
    } else {
        let staked_amount = std::cmp::max(a_poll.total_share_at_start_poll, total_share);
        Decimal::from_ratio(tallied_weight, staked_amount)
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
    }

    // Update poll indexer, remove from in progress and add to new poll status indexer
    poll_indexer_store(deps.storage, &PollStatus::InProgress).remove(&a_poll.id.to_be_bytes());
    poll_indexer_store(deps.storage, &poll_status).save(&a_poll.id.to_be_bytes(), &true)?;

    // Update poll status
    a_poll.status = poll_status;
    a_poll.total_share_at_end_poll = Some(total_share);
    poll_store(deps.storage).save(&poll_id.to_be_bytes(), &a_poll)?;

    Ok(Response::new().add_attributes(vec![
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
    quorum: Option<Decimal>,
    threshold: Option<Decimal>,
    voting_period: Option<u64>,
) -> Result<Response, ContractError> {
    let api = deps.api;
    config_store(deps.storage).update(|mut config| {
        if config.owner != api.addr_canonicalize(info.sender.as_str())? {
            return Err(ContractError::Unauthorized {});
        }
        if let Some(owner) = owner {
            config.owner = api.addr_canonicalize(&owner)?;
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
        Ok(config)
    })?;

    Ok(Response::new().add_attributes(vec![("action", "update_config")]))
}

/// delegate my share (
/// should not be currently voted in in progress polls
fn delegate_vote(
    deps: DepsMut,
    voter_id: String,
    delegator_id: String,
) -> Result<Response, ContractError> {
    // save in delegate to
    let voter_key = voter_id.as_bytes();
    let mut token_manager = bank_read(deps.storage)
        .may_load(voter_key)?
        .unwrap_or_default();

    // only leave in progress polls
    token_manager.locked_share.retain(|(poll_id, _)| {
        let poll: Poll = poll_read(deps.storage)
            .load(&poll_id.to_be_bytes())
            .unwrap();

        // remove voter if poll is not in progress
        if poll.status != PollStatus::InProgress {
            poll_voter_store(deps.storage, *poll_id).remove(voter_key);
        }

        poll.status == PollStatus::InProgress
    });

    // if voted in in progress polls
    if token_manager.locked_share.len() != 0 {
        return Err(ContractError::AlreadyVoted {});
    }

    // if already delegated to other
    if token_manager.delegate_to.is_some() {
        return Err(ContractError::AlreadyDelegated {});
    }
    token_manager.delegate_to = Some(delegator_id.clone());
    bank_store(deps.storage).save(voter_key, &token_manager)?;

    // save in delegate from
    let delegator_key = delegator_id.as_bytes();
    let mut token_manager = bank_read(deps.storage)
        .may_load(delegator_key)?
        .unwrap_or_default();
    token_manager.delegated_from.push(voter_id.clone());
    bank_store(deps.storage).save(delegator_key, &token_manager)?;

    Ok(Response::new().add_attributes(vec![
        ("action", "delegate"),
        ("from", voter_id.as_str()),
        ("to", delegator_id.as_str()),
    ]))
}

/// undelegate my share
fn undelegate_vote(deps: DepsMut, voter_id: String) -> Result<Response, ContractError> {
    let voter_key = voter_id.as_bytes();
    // delete delegate to
    let mut token_manager = bank_read(deps.storage)
        .may_load(voter_key)?
        .unwrap_or_default();

    // if not delegated to other
    if token_manager.delegate_to.is_none() {
        return Err(ContractError::NotYetDelegated {});
    }
    let delegator = token_manager.delegate_to.unwrap();

    token_manager.delegate_to = None;
    bank_store(deps.storage).save(voter_key, &token_manager)?;

    // delete in delegate from
    let delegator_key = delegator.as_bytes();
    let mut token_manager = bank_read(deps.storage)
        .may_load(delegator_key)?
        .unwrap_or_default();
    let delegated_from = token_manager.delegated_from.clone();
    let index = delegated_from
        .into_iter()
        .position(|x| x == voter_id)
        .unwrap();
    token_manager.delegated_from.swap_remove(index);

    bank_store(deps.storage).save(delegator_key, &token_manager)?;

    Ok(Response::new().add_attributes(vec![
        ("action", "undelegate"),
        ("from", voter_id.as_str()),
        ("to", delegator.as_str()),
    ]))
}

/// return maximum balance between in progress polls
fn compute_locked_balance(
    storage: &mut dyn Storage,
    token_manager: &mut TokenManager,
    voter_key: &[u8],
) -> u128 {
    // only leave in progress polls
    token_manager.locked_share.retain(|(poll_id, _)| {
        let poll: Poll = poll_read(storage).load(&poll_id.to_be_bytes()).unwrap();

        // remove voter if poll is not in progress
        if poll.status != PollStatus::InProgress {
            poll_voter_store(storage, *poll_id).remove(voter_key);
        }

        poll.status == PollStatus::InProgress
    });

    token_manager
        .locked_share
        .iter()
        .map(|(_, v)| v.balance.u128())
        .max()
        .unwrap_or_default()
}

fn cancel_vote(
    deps: DepsMut,
    env: Env,
    voter_id: String,
    poll_id: u64,
) -> Result<Response, ContractError> {
    let voter_key = voter_id.as_bytes();
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

    // check if sender_address has voted
    if !poll_voter_read(deps.storage, poll_id)
        .load(voter_key)
        .is_ok()
    {
        return Err(ContractError::NotYetVoted {});
    }

    let mut token_manager = bank_read(deps.storage)
        .may_load(voter_key)?
        .unwrap_or_default();

    let vote_info = poll_voter_read(deps.storage, poll_id).load(voter_key)?;

    // increment yes/no votes
    if vote_info.vote == VoteOption::Yes {
        a_poll.yes_votes -= vote_info.balance;
    } else {
        a_poll.no_votes -= vote_info.balance;
    }

    token_manager.locked_share.retain(|(id, _)| {
        let poll: Poll = poll_read(deps.storage).load(&id.to_be_bytes()).unwrap();

        // remove voter if poll is not in progress or poll is the same vote to cancel
        if poll.status != PollStatus::InProgress || *id == poll_id {
            poll_voter_store(deps.storage, *id).remove(voter_key);
        }

        poll.status == PollStatus::InProgress && *id != poll_id
    });

    bank_store(deps.storage).save(voter_key, &token_manager)?;

    poll_store(deps.storage).save(&poll_id.to_be_bytes(), &a_poll)?;

    Ok(Response::new().add_attributes(vec![
        ("action", "cancel_vote"),
        ("poll_id", poll_id.to_string().as_str()),
        ("amount", vote_info.balance.to_string().as_str()),
        ("voter", voter_id.as_str()),
        ("vote_option", vote_info.vote.to_string().as_str()),
    ]))
}

/// mint warrant tokens
/// only owner can mint
pub fn mint(
    deps: DepsMut,
    info: MessageInfo,
    recipient_id: String,
    amount: Uint128,
) -> Result<Response, ContractError> {
    let config: Config = config_store(deps.storage).load()?;
    let sender = info.sender;
    if config.owner != deps.api.addr_canonicalize(sender.as_str())? {
        return Err(ContractError::Unauthorized {});
    }

    if amount.is_zero() {
        return Err(ContractError::InsufficientFunds {});
    }

    _mint(deps.storage, recipient_id.as_bytes(), amount)?;

    Ok(Response::new().add_attributes(vec![
        ("action", "mint"),
        ("to", &recipient_id),
        ("amount", amount.to_string().as_str()),
    ]))
}

/// member can burn token all
fn exit(deps: DepsMut, sender_id: String) -> Result<Response, ContractError> {
    let key = sender_id.as_bytes();
    let token_manager = bank_read(deps.storage).may_load(key)?.unwrap_or_default();
    let amount = token_manager.balance;
    _burn(deps.storage, key, amount)?;
    Ok(Response::new().add_attributes(vec![
        ("action", "exit"),
        ("from", sender_id.as_str()),
        ("amount", &amount.to_string()),
    ]))
}

/// transfer from owner to recipient
/// only callable by owner address
/// amount: None (transfer all)
pub fn transfer_from(
    deps: DepsMut,
    info: MessageInfo,
    owner_id: String,
    recipient_id: String,
    amount: Uint128,
) -> Result<Response, ContractError> {
    let config: Config = config_store(deps.storage).load()?;
    let sender = info.sender.as_str();
    if config.owner != deps.api.addr_canonicalize(sender)? {
        return Err(ContractError::Unauthorized {});
    }
    let recipient_key = recipient_id.as_bytes();
    let owner_key = owner_id.as_bytes();

    if amount.is_zero() {
        return Err(ContractError::InsufficientFunds {});
    }

    _burn(deps.storage, owner_key, amount)?;
    _mint(deps.storage, recipient_key, amount)?;
    Ok(Response::new().add_attributes(vec![
        ("action", "transfer_from"),
        ("from", &owner_id),
        ("to", &recipient_id),
        ("by", sender),
        ("amount", amount.to_string().as_str()),
    ]))
}

/// mint warrant tokens
fn _mint(storage: &mut dyn Storage, key: &[u8], amount: Uint128) -> Result<(), ContractError> {
    let mut token_manager = bank_read(storage).may_load(key)?.unwrap_or_default();
    let mut state: State = state_store(storage).load()?;
    let old_share = token_manager.share;
    state.total_share -= old_share;
    token_manager.balance += amount;
    token_manager.share = token_manager.balance.isqrt();
    let new_share = token_manager.share;
    state.total_share += new_share;

    state_store(storage).save(&state)?;
    bank_store(storage).save(key, &token_manager)?;

    Ok(())
}

/// burn tokens (used in instant_burn, transfer(burn --> mint))
/// can burn only non locked shares
fn _burn(storage: &mut dyn Storage, key: &[u8], amount: Uint128) -> Result<(), ContractError> {
    if let Some(mut token_manager) = bank_read(storage).may_load(key)? {
        let mut state: State = state_store(storage).load()?;
        // Load total share & total balance except proposal deposit amount
        let locked_share = compute_locked_balance(storage, &mut token_manager, key);

        let balance = token_manager.balance.u128();
        let locked_amount = locked_share.pow(2);
        let withdraw_amount = amount.u128();
        if locked_amount + withdraw_amount > balance {
            Err(ContractError::InvalidWithdrawAmount {})
        } else {
            let old_share = token_manager.share;
            state.total_share -= old_share;
            token_manager.balance = Uint128::from(balance - withdraw_amount);
            token_manager.share = token_manager.balance.isqrt();
            let new_share = token_manager.share;
            state.total_share += new_share;
            bank_store(storage).save(key, &token_manager)?;
            state_store(storage).save(&state)?;
            Ok(())
        }
    } else {
        Err(ContractError::NothingStaked {})
    }
}
