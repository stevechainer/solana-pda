use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer};
use anchor_spl::associated_token::{AssociatedToken};
use std::convert::TryFrom;
use std::convert::TryInto;
use std::mem::size_of;

declare_id!("3fKWJqNzcLmrDVETM7AELXuPd1N21syu8EsoaUgnoLa4");

const YEAR_DURATION: u128 = 365*24*3600;
const APY: u128 = 50;//this mean 50 %


#[program]
pub mod solana_spltoken_staking {
    use super::*;

    pub fn create_state(
        _ctx: Context<CreateState>,
        token_per_second: u64,
    ) -> Result<()> {
        let state = &mut _ctx.accounts.state;
        state.authority = _ctx.accounts.authority.key();
        state.bump = _ctx.bumps.state;
        state.start_time = _ctx.accounts.clock.unix_timestamp;
        state.token_per_second = token_per_second;
        state.reward_mint = _ctx.accounts.reward_mint.key();
        state.reward_vault = _ctx.accounts.reward_vault.key();
        Ok(())
    }

    pub fn create_pool(
        _ctx: Context<CreateFarmPool>,
        point: u64,
        amount_multipler: u64,
    ) -> Result<()> {
        let state = &mut _ctx.accounts.state;
        let pool = &mut _ctx.accounts.pool;
        pool.bump = _ctx.bumps.pool;
        pool.mint = _ctx.accounts.mint.key();
        pool.vault = _ctx.accounts.vault.key();
        pool.point = point;
        pool.amount_multipler = amount_multipler;
        pool.authority = _ctx.accounts.authority.key();

        state.total_point = state.total_point.checked_add(point).unwrap();

        emit!(PoolCreated {
            pool: _ctx.accounts.pool.key(),
            mint: _ctx.accounts.mint.key()
        });
        Ok(())
    }

    pub fn fund_reward_token(_ctx: Context<Fund>, amount: u64) -> Result<()> {
        let cpi_accounts = Transfer {
            from: _ctx.accounts.user_vault.to_account_info(),
            to: _ctx.accounts.reward_vault.to_account_info(),
            authority: _ctx.accounts.authority.to_account_info(),
        };
        let cpi_program = _ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);
        token::transfer(cpi_ctx, amount)?;
        Ok(())
    }

    pub fn create_user(_ctx: Context<CreatePoolUser>) -> Result<()> {
        msg!("create_user_init");
        let user = &mut _ctx.accounts.user;
        user.authority = _ctx.accounts.authority.key();
        user.bump = _ctx.bumps.user;
        user.pool = _ctx.accounts.pool.key();

        let mut pool = &mut _ctx.accounts.pool;
        pool.total_user += 1;
        emit!(UserCreated {
            pool: _ctx.accounts.pool.key(),
            user: _ctx.accounts.user.key(),
            authority: _ctx.accounts.authority.key(),
        });
        msg!("create_user_done");
        Ok(())
    }

    pub fn stake(_ctx: Context<Stake>, amount: u64) -> Result<()> {
        msg!("stake_start");
        let state = &mut _ctx.accounts.state;
        let user = &mut _ctx.accounts.user;
        let pool = &mut _ctx.accounts.pool;
        let referral_user = &mut _ctx.accounts.referral_user;

        let deposit_amount = amount.checked_mul(95).unwrap().checked_div(100).unwrap();
        let referral_amount = u128::from(amount.checked_mul(5).unwrap().checked_div(100).unwrap());
        user.amount = user.amount.checked_add(deposit_amount).unwrap();
        pool.amount = pool.amount.checked_add(amount).unwrap();

        referral_user.extra_reward = referral_user.extra_reward.checked_add(referral_amount).unwrap();
        user.last_stake_time = _ctx.accounts.clock.unix_timestamp;

        let cpi_accounts = Transfer {
            from: _ctx.accounts.user_vault.to_account_info(),
            to: _ctx.accounts.pool_vault.to_account_info(),
            authority: _ctx.accounts.authority.to_account_info(),
        };
        let cpi_program = _ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);
        token::transfer(cpi_ctx, amount)?;
        emit!(UserStaked {
            pool: _ctx.accounts.pool.key(),
            user: _ctx.accounts.user.key(),
            authority: _ctx.accounts.authority.key(),
            amount
        });
        msg!("stake_end");
        Ok(())
    }

    pub fn unstake(_ctx: Context<Stake>, amount: u64) -> Result<()> {
        msg!("unstake_start");
        let state = &mut _ctx.accounts.state;
        let user = &mut _ctx.accounts.user;
        let pool = &mut _ctx.accounts.pool;

        require!(user.amount >= amount, ErrorCode::UnstakeOverAmount);
        require!(
            user.last_stake_time
                .checked_add(user.lock_duration)
                .unwrap()
                <= _ctx.accounts.clock.unix_timestamp,
            ErrorCode::UnderLocked
        );

        let seconds = _ctx.accounts.clock.unix_timestamp.checked_sub(user.last_stake_time).unwrap();

        let total_reward_amount: u128 = u128::from(user.amount)
            .checked_mul(APY)
            .unwrap()
            .checked_div(100)
            .unwrap()
            .checked_mul(seconds as u128)
            .unwrap()
            .checked_div(YEAR_DURATION)
            .unwrap();
        
        user.reward_amount = user.reward_amount
            .checked_add(total_reward_amount)
            .unwrap();


        user.last_stake_time = _ctx.accounts.clock.unix_timestamp;
        user.amount = user.amount.checked_sub(amount).unwrap();
        pool.amount = pool.amount.checked_sub(amount).unwrap();

        let new_pool = &_ctx.accounts.pool;
        let cpi_accounts = Transfer {
            from: _ctx.accounts.pool_vault.to_account_info(),
            to: _ctx.accounts.user_vault.to_account_info(),
            authority: _ctx.accounts.pool.to_account_info(),
        };

        let seeds = &[new_pool.mint.as_ref(), &[new_pool.bump]];
        let signer = &[&seeds[..]];
        let cpi_program = _ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer);
        token::transfer(cpi_ctx, amount)?;
        emit!(UserUnstaked {
            pool: _ctx.accounts.pool.key(),
            user: _ctx.accounts.user.key(),
            authority: _ctx.accounts.authority.key(),
            amount
        });
        msg!("stake_end");
        Ok(())
    }

    pub fn harvest(_ctx: Context<Harvest>) -> Result<()> {
        msg!("claim_start");
        let state = &mut _ctx.accounts.state;
        let pool = &mut _ctx.accounts.pool;
        let user = &mut _ctx.accounts.user;

        let seconds =  _ctx.accounts.clock.unix_timestamp.checked_sub(user.last_stake_time).unwrap();
        let until_new_reward_amount: u128 = u128::from(user.amount)
            .checked_mul(APY)
            .unwrap()
            .checked_div(100)
            .unwrap()
            .checked_mul(seconds as u128)
            .unwrap()
            .checked_div(YEAR_DURATION)
            .unwrap();

        let total_reward = user.reward_amount.checked_add(user.extra_reward).unwrap().checked_add(until_new_reward_amount).unwrap().try_into().unwrap();

        let cpi_accounts = Transfer {
            from: _ctx.accounts.reward_vault.to_account_info(),
            to: _ctx.accounts.user_vault.to_account_info(),
            authority: state.to_account_info(),
        };

        let seeds = &[b"state".as_ref(), &[state.bump]];
        let signer = &[&seeds[..]];
        let cpi_program = _ctx.accounts.token_program.to_account_info();
        let cpi_ctx = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer);
        token::transfer(cpi_ctx, total_reward)?;

        user.reward_amount = 0;
        user.extra_reward = 0;
        user.last_stake_time = _ctx.accounts.clock.unix_timestamp;

        emit!(UserHarvested {
            pool: _ctx.accounts.pool.key(),
            user: _ctx.accounts.user.key(),
            authority: _ctx.accounts.authority.key(),
            amount: total_reward
        });
        msg!("claim_end");
        Ok(())
    }
}

#[derive(Accounts)]
#[instruction(bump: u8)]
pub struct CreateState<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(
        init,
        seeds = [b"state".as_ref()],
        bump,
        payer = authority,
        space = 8 + size_of::<StateAccount>()
    )]
    pub state: Account<'info, StateAccount>,

    #[account(
        init,
        associated_token::mint=reward_mint,
        associated_token::authority=state,
        payer = authority,
    )]
    pub reward_vault: Account<'info, TokenAccount>,
    pub reward_mint: Box<Account<'info, Mint>>,
    pub system_program: Program<'info, System>,
    #[account(constraint = token_program.key == &token::ID)]
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub clock: Sysvar<'info, Clock>,
    pub rent: Sysvar<'info, Rent>
}

#[derive(Accounts)]
#[instruction(bump: u8)]
pub struct CreateFarmPool<'info> {
    #[account(
        init,
        seeds = [mint.key().as_ref()],
        bump,
        payer = authority,
        space = 8 + size_of::<FarmPoolAccount>()
    )]
    pub pool: Account<'info, FarmPoolAccount>,
    #[account(mut, seeds = [b"state".as_ref()], bump = state.bump, has_one = authority)]
    pub state: Account<'info, StateAccount>,
    pub mint: Box<Account<'info, Mint>>,
    #[account(
        init,
        associated_token::mint=mint,
        associated_token::authority=pool,
        payer = authority,
    )]
    pub vault: Account<'info, TokenAccount>,
    #[account(mut)]
    pub authority: Signer<'info>,
    pub system_program: Program<'info, System>,
    #[account(constraint = token_program.key == &token::ID)]
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub clock: Sysvar<'info, Clock>,
}

#[derive(Accounts)]
pub struct Fund<'info> {
    #[account(mut, seeds = [b"state".as_ref()], bump = state.bump)]
    pub state: Account<'info, StateAccount>,
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(mut, constraint = reward_vault.owner == state.key())]
    pub reward_vault: Box<Account<'info, TokenAccount>>,
    #[account(mut, constraint = user_vault.owner == authority.key())]
    pub user_vault: Box<Account<'info, TokenAccount>>,
    #[account(constraint = token_program.key == &token::ID)]
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct CreatePoolUser<'info> {
    #[account(
        init,
        seeds = [pool.key().as_ref(), authority.key().as_ref()],
        bump,
        payer = authority,
        space = 8 + size_of::<FarmPoolUserAccount>()
    )]
    pub user: Account<'info, FarmPoolUserAccount>,
    #[account(mut, seeds = [b"state".as_ref()], bump = state.bump)]
    pub state: Account<'info, StateAccount>,
    #[account(mut, seeds = [pool.mint.key().as_ref()], bump = pool.bump)]
    pub pool: Account<'info, FarmPoolAccount>,
    #[account(mut)]
    pub authority: Signer<'info>,
    pub system_program: Program<'info, System>,
    #[account(constraint = token_program.key == &token::ID)]
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct Stake<'info> {
    #[account(mut, seeds = [pool.key().as_ref(), authority.key().as_ref()], bump = user.bump, has_one = pool, has_one = authority)]
    pub user: Account<'info, FarmPoolUserAccount>,
    #[account(mut, seeds = [b"state".as_ref()], bump = state.bump)]
    pub state: Account<'info, StateAccount>,
    #[account(mut, seeds = [pool.mint.key().as_ref()], bump = pool.bump)]
    pub pool: Account<'info, FarmPoolAccount>,
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(constraint = mint.key() == pool.mint)]
    pub mint: Box<Account<'info, Mint>>,
    #[account(mut, constraint = pool_vault.owner == pool.key())]
    pub pool_vault: Box<Account<'info, TokenAccount>>,
    #[account(mut, constraint = user_vault.owner == authority.key())]
    pub user_vault: Box<Account<'info, TokenAccount>>,
    /// CHECK:
    #[account(mut)]
    pub referral: AccountInfo<'info>,
    #[account(mut, seeds = [pool.key().as_ref(), referral.key().as_ref()], bump = referral_user.bump, has_one = pool)]
    pub referral_user: Account<'info, FarmPoolUserAccount>,
    pub system_program: Program<'info, System>,
    #[account(constraint = token_program.key == &token::ID)]
    pub token_program: Program<'info, Token>,
    pub clock: Sysvar<'info, Clock>,
}

#[derive(Accounts)]
pub struct Harvest<'info> {
    #[account(mut, seeds = [pool.key().as_ref(), authority.key().as_ref()], bump = user.bump, has_one = pool, has_one = authority)]
    pub user: Account<'info, FarmPoolUserAccount>,
    #[account(mut, seeds = [b"state".as_ref()], bump = state.bump)]
    pub state: Account<'info, StateAccount>,
    #[account(mut, seeds = [pool.mint.key().as_ref()], bump = pool.bump)]
    pub pool: Account<'info, FarmPoolAccount>,
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(constraint = mint.key() == pool.mint)]
    pub mint: Box<Account<'info, Mint>>,
    #[account(mut, constraint = reward_vault.owner == state.key())]
    pub reward_vault: Box<Account<'info, TokenAccount>>,
    #[account(mut, constraint = user_vault.owner == authority.key())]
    pub user_vault: Box<Account<'info, TokenAccount>>,
    pub system_program: Program<'info, System>,
    #[account(constraint = token_program.key == &token::ID)]
    pub token_program: Program<'info, Token>,
    pub clock: Sysvar<'info, Clock>,
}

#[account]
#[derive(Default)]
pub struct StateAccount {
    pub authority: Pubkey,
    pub reward_mint: Pubkey,
    pub reward_vault: Pubkey,
    pub bump: u8,
    pub total_point: u64,
    pub start_time: i64,
    pub token_per_second: u64,
}

#[account]
#[derive(Default)]
pub struct FarmPoolAccount {
    pub bump: u8,
    pub authority: Pubkey,
    pub amount: u64,
    pub mint: Pubkey,
    pub vault: Pubkey,
    pub point: u64,
    pub last_reward_time: i64,
    pub acc_reward_per_share: u128,
    pub amount_multipler: u64,
    pub total_user: u64,
}

impl FarmPoolAccount {
    fn update<'info>(&mut self, state: &StateAccount, clock: &Sysvar<'info, Clock>) -> Result<()> {
        let seconds = u128::try_from(
            clock
                .unix_timestamp
                .checked_sub(self.last_reward_time)
                .unwrap(),
        )
        .unwrap();
        let mut reward_per_share: u128 = 0;
        if self.amount > 0 && seconds > 0 && self.point > 0 {
            reward_per_share = u128::from(state.token_per_second)
                .checked_mul(seconds)
                .unwrap()
                .checked_mul(u128::from(self.point))
                .unwrap()
                .checked_mul(YEAR_DURATION)
                .unwrap()
                .checked_div(u128::from(state.total_point))
                .unwrap()
                .checked_div(u128::from(self.amount))
                .unwrap();
        }
        self.acc_reward_per_share = self
            .acc_reward_per_share
            .checked_add(reward_per_share)
            .unwrap();
        self.last_reward_time = clock.unix_timestamp;

        Ok(())
    }
}

#[account]
#[derive(Default)]
pub struct FarmPoolUserAccount {
    pub bump: u8,
    pub pool: Pubkey,
    pub authority: Pubkey,
    pub amount: u64,
    pub reward_amount: u128,
    pub extra_reward: u128, // extra from lock duration; ex lock 12M => +10%
    pub reward_debt: u128,
    pub last_stake_time: i64,
    pub lock_duration: i64,
    pub reserved_1: u128,
    pub reserved_2: u128,
    pub reserved_3: u128,
}

impl FarmPoolUserAccount {
    fn calculate_reward_amount<'info>(
        &mut self,
        pool: &FarmPoolAccount,
    ) -> Result<()> {
        let pending_amount: u128 = u128::from(self.amount)
            .checked_mul(pool.acc_reward_per_share)
            .unwrap()
            .checked_div(YEAR_DURATION)
            .unwrap()
            .checked_sub(u128::from(self.reward_debt))
            .unwrap();
        self.reward_amount = self.reward_amount.checked_add(pending_amount).unwrap();
        Ok(())
    }
    fn calculate_reward_debt<'info>(&mut self, pool: &FarmPoolAccount) -> Result<()> {

        msg!("multiplied {}", u128::from(self.amount).checked_mul(pool.acc_reward_per_share).unwrap());
        msg!("scaled {}", u128::from(self.amount).checked_mul(pool.acc_reward_per_share).unwrap().checked_div(YEAR_DURATION).unwrap());

        self.reward_debt = u128::from(self.amount)
            .checked_mul(pool.acc_reward_per_share)
            .unwrap()
            .checked_div(YEAR_DURATION)
            .unwrap();
        Ok(())
    }
}

#[error_code]
pub enum ErrorCode {
    #[msg("Over staked amount")]
    UnstakeOverAmount,
    #[msg("Under locked")]
    UnderLocked,
    #[msg("Pool is working")]
    WorkingPool,
    #[msg("Invalid Lock Duration")]
    InvalidLockDuration,
    #[msg("Invalid SEQ")]
    InvalidSEQ,
}
#[event]
pub struct PoolCreated {
    pool: Pubkey,
    mint: Pubkey,
}
#[event]
pub struct UserCreated {
    pool: Pubkey,
    user: Pubkey,
    authority: Pubkey,
}
#[event]
pub struct UserStaked {
    pool: Pubkey,
    user: Pubkey,
    authority: Pubkey,
    amount: u64
}
#[event]
pub struct UserUnstaked {
    pool: Pubkey,
    user: Pubkey,
    authority: Pubkey,
    amount: u64,
}
#[event]
pub struct UserHarvested {
    pool: Pubkey,
    user: Pubkey,
    authority: Pubkey,
    amount: u64,
}
