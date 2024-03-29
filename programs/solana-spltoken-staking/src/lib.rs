use anchor_lang::prelude::*;
use anchor_lang::system_program::{transfer, Transfer};

declare_id!("92DVkqDr2R3aKW5gmguYkAGi6jY32gSkGWHYMtkLQYHu");

#[program]
mod w2e_game {
    use super::*;

    pub fn deposit_to_pda(ctx: Context<Fund>, fund_lamports: u64) -> Result<()> {
        let pda = &mut ctx.accounts.pda;
        let signer = &mut ctx.accounts.signer;
        let system_program = &ctx.accounts.system_program;

        let pda_balance_before = pda.get_lamports();

        transfer(
            CpiContext::new(
                system_program.to_account_info(),
                Transfer {
                    from: signer.to_account_info(),
                    to: pda.to_account_info(),
                },
            ),
            fund_lamports,
        )?;

        let pda_balance_after = pda.get_lamports();

        require_eq!(pda_balance_after, pda_balance_before + fund_lamports);

        Ok(())
    }

    pub fn withdraw_from_pda(ctx: Context<Fund>, return_lamports: u64) -> Result<()> {
        let pda = &mut ctx.accounts.pda;
        let user = &mut ctx.accounts.user;
        let system_program = &ctx.accounts.system_program;

        let pda_balance_before = pda.get_lamports();

        let bump = &[ctx.bumps.pda];
        let seeds: &[&[u8]] = &[b"vault".as_ref(), bump];
        let signer_seeds = &[&seeds[..]];

        transfer(
            CpiContext::new(
                system_program.to_account_info(),
                Transfer {
                    from: pda.to_account_info(),
                    to: user.to_account_info(),
                },
            ).with_signer(signer_seeds),
            return_lamports,
        )?;

        let pda_balance_after = pda.get_lamports();

        require_eq!(pda_balance_after, pda_balance_before - return_lamports);

        Ok(())
    }

}

#[account]
pub struct UserConfig {
    // User account data fields
    pub admin: Pubkey,
    // Add other fields here
}

#[derive(Accounts)]
pub struct Fund<'info> {
    #[account(
        mut,
        seeds = [b"vault".as_ref()],
        bump
    )]
    pub pda: SystemAccount<'info>,

    #[account(mut)]
    pub signer: Signer<'info>,

    #[account(mut)]
    pub user: Account<'info, UserConfig>,

    pub system_program: Program<'info, System>,
}
