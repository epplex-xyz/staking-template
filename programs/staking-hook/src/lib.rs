pub use anchor_lang::{
    prelude::*,
    system_program::{ create_account, CreateAccount },
    solana_program::program_memory::sol_memcpy,
};

pub use spl_tlv_account_resolution::{
    account::ExtraAccountMeta, seeds::Seed, state::ExtraAccountMetaList,
};

pub use anchor_spl::token_interface::{ Mint, TokenAccount, TokenInterface };

pub use spl_transfer_hook_interface::instruction::{ExecuteInstruction, TransferHookInstruction};

declare_id!("EB9buE9e2Gwphxs6iPKyzXjqnom7vSQgDj2PGBiP7X4h");

#[program]
pub mod staking_hook {
    use super::*;

    pub fn initialize_extra_account_meta_list(
        ctx: Context<InitializeExtraAccountMetaList>,
    ) -> Result<()> {
        // index 0-3 are the accounts required for token transfer (source, mint, destination, owner)
        // index 4 is address of ExtraAccountMetaList account
        let account_metas = vec![
        // index 5, sysvar_instruction
        ExtraAccountMeta::new_with_seeds(
            &[
                Seed::Literal { bytes: "staking".as_bytes().to_vec() },
                Seed::AccountKey { index: 1 } 
            ],
            false, // is_signer
            true,  // is_writable
        )?,
        ];
    
        // calculate account size
        let account_size = ExtraAccountMetaList::size_of(account_metas.len())? as u64;
        // calculate minimum required lamports
        let lamports = Rent::get()?.minimum_balance(account_size as usize);
    
        let mint = ctx.accounts.mint.key();
        let signer_seeds: &[&[&[u8]]] = &[&[
            b"extra-account-metas",
            &mint.as_ref(),
            &[ctx.bumps.extra_account_meta_list],
        ]];
    
        // create ExtraAccountMetaList account
        create_account(
            CpiContext::new(
                ctx.accounts.system_program.to_account_info(),
                CreateAccount {
                    from: ctx.accounts.payer.to_account_info(),
                    to: ctx.accounts.extra_account_meta_list.to_account_info(),
                },
            )
            .with_signer(signer_seeds),
            lamports,
            account_size,
            ctx.program_id,
        )?;
    
        // initialize ExtraAccountMetaList account with extra accounts
        ExtraAccountMetaList::init::<ExecuteInstruction>(
            &mut ctx.accounts.extra_account_meta_list.try_borrow_mut_data()?,
            &account_metas,
        )?;
    
        Ok(())
    }

    pub fn stake(ctx: Context<Stake>) -> Result<()> {
       
        let info = ctx.accounts.staking_account.to_account_info(); 
        let data = info.try_borrow_mut_data()?;

        match  StakingData::try_deserialize(&mut &data[..]) {
            Ok(_) => {
                require!(ctx.accounts.staking_account.starting_time == 0, StakingErr::AlreadyStaked);
                ctx.accounts.staking_account.starting_time = Clock::get()?.unix_timestamp;
            },
            Err(_) => {
                ctx.accounts.staking_account.set_inner(
                    StakingData {
                        starting_time: Clock::get()?.unix_timestamp,
                        time: 0,
                    }
                );
            }
        }
        
        Ok(())
    }

    pub fn transfer_hook(ctx: Context<TransferHook>, amount: u64) -> Result<()> {

        let info = ctx.accounts.staking_account.to_account_info();
        let mut data = info.try_borrow_mut_data()?;

        // Try and Deserialize the Account
        match  StakingData::try_deserialize(&mut &data[..]) {
            Ok(mut staking_account) => {
                if staking_account.starting_time != 0 {
                    
                    // Update time and Unstake the NFT
                    staking_account.time = Clock::get()?.unix_timestamp - staking_account.starting_time;
                    staking_account.starting_time = 0;

                    // Serialize it back and update the account
                    let mut writer = &mut data[..];
                    staking_account.try_serialize(&mut writer)?;
                }
            },
            Err(_) => {
            // Do nothing
            }
        }
        
        Ok(())
    }

    // fallback instruction handler as workaround to anchor instruction discriminator check
    pub fn fallback<'info>(
        program_id: &Pubkey,
        accounts: &'info [AccountInfo<'info>],
        data: &[u8],
    ) -> Result<()> {
        let instruction = TransferHookInstruction::unpack(data)?;

        // match instruction discriminator to transfer hook interface execute instruction
        // token2022 program CPIs this instruction on token transfer
        match instruction {
            TransferHookInstruction::Execute { amount } => {
                let amount_bytes = amount.to_le_bytes();

                // invoke custom transfer hook instruction on our program
                __private::__global::transfer_hook(program_id, accounts, &amount_bytes)
            }
            _ => return Err(ProgramError::InvalidInstructionData.into()),
        }
    }
    
    
}

#[derive(Accounts)]
pub struct InitializeExtraAccountMetaList<'info> {
    #[account(mut)]
    payer: Signer<'info>,
    /// CHECK: ExtraAccountMetaList Account, must use these seeds
    #[account(
        mut,
        seeds = [b"extra-account-metas", mint.key().as_ref()],
        bump
    )]
    pub extra_account_meta_list: AccountInfo<'info>,
    pub mint: InterfaceAccount<'info, Mint>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct TransferHook<'info> {
    #[account(
        token::mint = mint,
        token::authority = owner,
    )]
    pub source_token: InterfaceAccount<'info, TokenAccount>,
    pub mint: InterfaceAccount<'info, Mint>,
    #[account(
        token::mint = mint,
    )]
    pub destination_token: InterfaceAccount<'info, TokenAccount>,
    /// CHECK: source token account owner, can be SystemAccount or PDA owned by another program
    pub owner: UncheckedAccount<'info>,
    /// CHECK: ExtraAccountMetaList Account,
    #[account(
        seeds = [b"extra-account-metas", mint.key().as_ref()],
        bump
    )]
    pub extra_account_meta_list: UncheckedAccount<'info>,
    #[account(
        mut,
        seeds = [b"staking", mint.key().as_ref()],
        bump
    )]
    /// CHECK: Sysvar instruction account
    pub staking_account: UncheckedAccount<'info>,
}

#[derive(Accounts)]
pub struct Stake<'info> {
    #[account(mut)]
    staker: Signer<'info>,

    pub mint: InterfaceAccount<'info, Mint>,
     #[account(
        token::mint = mint,
        token::authority = staker,
    )]
    pub token: InterfaceAccount<'info, TokenAccount>,
    
    #[account(
        init_if_needed,
        payer = staker,
        space = StakingData::INIT_SPACE,
        seeds = [b"staking", mint.key().as_ref()],
        bump
    )]
    pub staking_account: Account<'info, StakingData>,

    pub system_program: Program<'info, System>,
}

#[account]
pub struct StakingData {
    pub starting_time: i64,
    pub time: i64,
}

impl Space for StakingData {
    const INIT_SPACE: usize = 8 + 8 + 8;
}

#[error_code]
pub enum StakingErr {
    #[msg("This NFT is already staked.")]
    AlreadyStaked,
}
