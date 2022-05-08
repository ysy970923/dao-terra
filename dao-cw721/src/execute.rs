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
            ExecuteMsg::TransferNft {
                recipient,
                token_id,
            } => self.transfer_nft(deps, env, info, recipient, token_id),
            ExecuteMsg::ExecuteDAO {
                token_id,
                msg,
            } => self.execute_dao(deps, env, info, token_id, msg),
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

    // only owner of the contract can transfer
    fn transfer_nft(
        &self,
        deps: DepsMut,
        env: Env,
        info: MessageInfo,
        recipient: String,
        token_id: String,
    ) -> Result<Response<C>, ContractError> {
        let owner_address = self.owner.load(deps.storage)?;

        if info.sender != owner_address {
            return Err(ContractError::Unauthorized {});
        }
        let mut token = self.tokens.load(deps.storage, &token_id)?;
        let old_owner = token.owner;
        // set owner and remove existing approvals
        token.owner = deps.api.addr_validate(&recipient)?;
        self.tokens.save(deps.storage, &token_id, &token)?;
        Ok(Response::new()
            .add_attribute("action", "transfer_nft")
            .add_attribute("from", old_owner)
            .add_attribute("to", recipient)
            .add_attribute("token_id", token_id))
    }

    fn execute_dao(
        &self,
        deps: DepsMut,
        env: Env,
        info: MessageInfo,
        token_id: String,
        msg: Binary,
    ) -> Result<Response<C>, ContractError> {
        let token = self.tokens.load(deps.storage, &token_id)?;
        let gov_contract = self.gov_contract.load(deps.storage)?;
        let sender = info.sender;
        if token.owner != sender {
            return Err(ContractError::Unauthorized {});
        }
        let send = Cw721ReceiveMsg {
            sender: sender.to_string(),
            token_id: token_id.clone(),
            msg,
        };

        Ok(Response::new()
            .add_message(send.into_cosmos_msg(gov_contract.clone())?)
            .add_attribute("action", "execute_dao")
            .add_attribute("sender", sender)
            .add_attribute("token_id", token_id))
    }
}
