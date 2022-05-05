use serde::de::DeserializeOwned;
use serde::Serialize;

use cosmwasm_std::{
    to_binary, Binary, CosmosMsg, Deps, DepsMut, Env, MessageInfo, Response, StdResult, WasmMsg,
};

use cw2::set_contract_version;
use cw721::{ContractInfoResponse, CustomMsg, Cw721Execute, Cw721ReceiveMsg, Expiration};

use crate::error::ContractError;
use crate::msg::{ExecuteMsg, InstantiateMsg, MintMsg};
use crate::state::{Approval, Cw721Contract, TokenInfo};

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:cw721-base";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

impl<'a, T, C> Cw721Contract<'a, T, C>
where
    T: Serialize + DeserializeOwned + Clone,
    C: CustomMsg,
{
    pub fn instantiate(
        &self,
        deps: DepsMut,
        _env: Env,
        _info: MessageInfo,
        msg: InstantiateMsg,
    ) -> StdResult<Response<C>> {
        set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

        let info = ContractInfoResponse {
            name: msg.name,
            symbol: msg.symbol,
        };
        self.contract_info.save(deps.storage, &info)?;
        let owner = deps.api.addr_validate(&msg.owner)?;
        self.owner.save(deps.storage, &owner)?;
        Ok(Response::default())
    }

    pub fn execute(
        &self,
        deps: DepsMut,
        env: Env,
        info: MessageInfo,
        msg: ExecuteMsg<T>,
    ) -> Result<Response<C>, ContractError> {
        match msg {
            ExecuteMsg::Mint(msg) => self.mint(deps, env, info, msg),
            ExecuteMsg::Approve {
                spender,
                token_id,
                expires,
            } => self.approve(deps, env, info, spender, token_id, expires),
            ExecuteMsg::Revoke { spender, token_id } => {
                self.revoke(deps, env, info, spender, token_id)
            }
            ExecuteMsg::ApproveAll { operator, expires } => {
                self.approve_all(deps, env, info, operator, expires)
            }
            ExecuteMsg::RevokeAll { operator } => self.revoke_all(deps, env, info, operator),
            ExecuteMsg::TransferNft {
                recipient,
                token_id,
            } => self.transfer_nft(deps, env, info, recipient, token_id),
            ExecuteMsg::SendNft {
                contract,
                token_id,
                msg,
            } => self.send_nft(deps, env, info, contract, token_id, msg),
        }
    }
}

// TODO pull this into some sort of trait extension??
impl<'a, T, C> Cw721Contract<'a, T, C>
where
    T: Serialize + DeserializeOwned + Clone,
    C: CustomMsg,
{
    /// only owner can mint
    pub fn mint(
        &self,
        deps: DepsMut,
        _env: Env,
        info: MessageInfo,
        msg: MintMsg<T>,
    ) -> Result<Response<C>, ContractError> {
        let owner = self.owner.load(deps.storage)?;

        if info.sender != owner {
            return Err(ContractError::Unauthorized {});
        }

        // create the token
        let token = TokenInfo {
            owner: deps.api.addr_validate(&msg.owner)?,
            approvals: vec![],
            token_uri: msg.token_uri,
            extension: msg.extension,
        };
        self.tokens
            .update(deps.storage, &msg.token_id, |old| match old {
                Some(_) => Err(ContractError::Claimed {}),
                None => Ok(token),
            })?;

        self.increment_tokens(deps.storage)?;

        Ok(Response::new()
            .add_attribute("action", "mint")
            .add_attribute("minter", info.sender)
            .add_attribute("token_id", msg.token_id))
    }
}

impl<'a, T, C> Cw721Execute<T, C> for Cw721Contract<'a, T, C>
where
    T: Serialize + DeserializeOwned + Clone,
    C: CustomMsg,
{
    type Err = ContractError;

    // only owner can transfer
    fn transfer_nft(
        &self,
        deps: DepsMut,
        env: Env,
        info: MessageInfo,
        recipient: String,
        token_id: String,
    ) -> Result<Response<C>, ContractError> {
        self._transfer_nft(deps, &env, &info, &recipient, &token_id)?;
        Ok(Response::new()
            .add_attribute("action", "transfer_nft")
            .add_attribute("sender", info.sender)
            .add_attribute("recipient", recipient)
            .add_attribute("token_id", token_id))
    }

    fn send_nft(
        &self,
        deps: DepsMut,
        env: Env,
        info: MessageInfo,
        contract: String,
        token_id: String,
        msg: Binary,
    ) -> Result<Response<C>, ContractError> {
        Err(ContractError::InvalidExecute {})
    }

    fn approve(
        &self,
        deps: DepsMut,
        env: Env,
        info: MessageInfo,
        spender: String,
        token_id: String,
        expires: Option<Expiration>,
    ) -> Result<Response<C>, ContractError> {
        Err(ContractError::InvalidExecute {})
    }

    fn revoke(
        &self,
        deps: DepsMut,
        env: Env,
        info: MessageInfo,
        spender: String,
        token_id: String,
    ) -> Result<Response<C>, ContractError> {
        Err(ContractError::InvalidExecute {})
    }

    fn approve_all(
        &self,
        deps: DepsMut,
        env: Env,
        info: MessageInfo,
        operator: String,
        expires: Option<Expiration>,
    ) -> Result<Response<C>, ContractError> {
        Err(ContractError::InvalidExecute {})
    }

    fn revoke_all(
        &self,
        deps: DepsMut,
        _env: Env,
        info: MessageInfo,
        operator: String,
    ) -> Result<Response<C>, ContractError> {
        Err(ContractError::InvalidExecute {})
    }
}

// helpers
impl<'a, T, C> Cw721Contract<'a, T, C>
where
    T: Serialize + DeserializeOwned + Clone,
    C: CustomMsg,
{
    pub fn _transfer_nft(
        &self,
        deps: DepsMut,
        env: &Env,
        info: &MessageInfo,
        recipient: &str,
        token_id: &str,
    ) -> Result<TokenInfo<T>, ContractError> {
        let owner = self.owner.load(deps.storage)?;

        if info.sender != owner {
            return Err(ContractError::Unauthorized {});
        }
        let mut token = self.tokens.load(deps.storage, &token_id)?;

        // set owner and remove existing approvals
        token.owner = deps.api.addr_validate(recipient)?;
        token.approvals = vec![];
        self.tokens.save(deps.storage, &token_id, &token)?;
        Ok(token)
    }
}
