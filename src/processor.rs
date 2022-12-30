use crate::error::StudentIntroError;
use crate::instruction::IntroInstruction;
use crate::state::{Reply, ReplyCounter, StudentInfo};
use borsh::BorshSerialize;
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    borsh::try_from_slice_unchecked,
    entrypoint::ProgramResult,
    msg,
    native_token::LAMPORTS_PER_SOL,
    program::invoke_signed,
    program_error::ProgramError,
    program_pack::IsInitialized,
    pubkey::Pubkey,
    system_instruction,
    system_program::ID as SYSTEM_PROGRAM_ID,
    sysvar::{rent::Rent, rent::ID as RENT_PROGRAM_ID, Sysvar},
};
use spl_associated_token_account::get_associated_token_address;
use spl_token::{
    instruction::{initialize_mint, mint_to},
    ID as TOKEN_PROGRAM_ID,
};
use std::convert::TryInto;

pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    let instruction = IntroInstruction::unpack(instruction_data)?;
    match instruction {
        IntroInstruction::InitUserInput { name, message } => {
            add_student_intro(program_id, accounts, name, message)
        }
        IntroInstruction::UpdateStudentIntro { name, message } => {
            update_student_intro(program_id, accounts, name, message)
        }
        IntroInstruction::AddReply { reply } => add_reply(program_id, accounts, reply),
        IntroInstruction::InitializeMint => initialize_token_mint(program_id, accounts),
    }
}

pub fn add_student_intro(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    name: String,
    message: String,
) -> ProgramResult {
    msg!("Adding student intro...");
    msg!("Name: {}", name);
    msg!("Message: {}", message);
    let account_info_iter = &mut accounts.iter();

    let initializer = next_account_info(account_info_iter)?;
    let user_account = next_account_info(account_info_iter)?;
    let reply_counter = next_account_info(account_info_iter)?;
    let token_mint_pda = next_account_info(account_info_iter)?;
    let mint_auth_pda = next_account_info(account_info_iter)?;
    let user_ata = next_account_info(account_info_iter)?;
    let system_program = next_account_info(account_info_iter)?;
    let token_program = next_account_info(account_info_iter)?;

    if !initializer.is_signer {
        msg!("Missing required signature");
        return Err(ProgramError::MissingRequiredSignature);
    }

    let (user_pda, bump_seed) =
        Pubkey::find_program_address(&[initializer.key.as_ref()], program_id);
    if user_pda != *user_account.key {
        msg!("Invalid seeds for PDA");
        return Err(StudentIntroError::InvalidPDA.into());
    }

    msg!("Deriving mint and mint authority");
    let (expected_mint_pda, _mint_bump) =
        Pubkey::find_program_address(&[b"token_mint"], program_id);
    let (expected_auth_pda, auth_bump) = Pubkey::find_program_address(&[b"token_auth"], program_id);

    if *token_mint_pda.key != expected_mint_pda {
        msg!("Incorrect token mint");
        return Err(StudentIntroError::IncorrectAccountError.into());
    }

    if *mint_auth_pda.key != expected_auth_pda {
        msg!("Incorrect token auth");
        return Err(StudentIntroError::IncorrectAccountError.into());
    }

    if *user_ata.key != get_associated_token_address(initializer.key, token_mint_pda.key) {
        msg!("Incorrect token mint");
        return Err(StudentIntroError::IncorrectAccountError.into());
    }

    if *token_program.key != TOKEN_PROGRAM_ID {
        msg!("Incorrect token program");
        return Err(StudentIntroError::IncorrectAccountError.into());
    }

    let studentinfo_discriminator = "studentinfo";
    let account_len: usize = 1000;
    let total_len: usize =
        (4 + studentinfo_discriminator.len()) + 1 + (4 + name.len()) + (4 + message.len());
    if total_len > account_len {
        msg!("Data length is larger than 1000 bytes");
        return Err(StudentIntroError::InvalidDataLength.into());
    }

    let rent = Rent::get()?;
    let rent_lamports = rent.minimum_balance(account_len);

    invoke_signed(
        &system_instruction::create_account(
            initializer.key,
            user_account.key,
            rent_lamports,
            account_len.try_into().unwrap(),
            program_id,
        ),
        &[
            initializer.clone(),
            user_account.clone(),
            system_program.clone(),
        ],
        &[&[initializer.key.as_ref(), &[bump_seed]]],
    )?;

    msg!("PDA created: {}", user_pda);

    msg!("unpacking state account");
    let mut account_data =
        try_from_slice_unchecked::<StudentInfo>(&user_account.data.borrow()).unwrap();
    msg!("borrowed account data");

    msg!("checking if studentinfo account is already initialized");
    if account_data.is_initialized() {
        msg!("Account already initialized");
        return Err(ProgramError::AccountAlreadyInitialized);
    }

    account_data.discriminator = studentinfo_discriminator.to_string();
    account_data.name = name;
    account_data.msg = message;
    account_data.is_initialized = true;
    msg!("serializing account");
    account_data.serialize(&mut &mut user_account.data.borrow_mut()[..])?;
    msg!("state account serialized");

    msg!("create reply counter");
    let counter_discriminator = "counter";
    let counter_len: usize = (4 + counter_discriminator.len()) + 1 + 1;

    let rent = Rent::get()?;
    let counter_rent_lamports = rent.minimum_balance(counter_len);

    let (counter, counter_bump) =
        Pubkey::find_program_address(&[user_pda.as_ref(), "reply".as_ref()], program_id);
    if counter != *reply_counter.key {
        msg!("Invalid seeds for PDA");
        return Err(ProgramError::InvalidArgument);
    }

    invoke_signed(
        &system_instruction::create_account(
            initializer.key,
            reply_counter.key,
            counter_rent_lamports,
            counter_len.try_into().unwrap(),
            program_id,
        ),
        &[
            initializer.clone(),
            reply_counter.clone(),
            system_program.clone(),
        ],
        &[&[user_pda.as_ref(), "reply".as_ref(), &[counter_bump]]],
    )?;
    msg!("reply counter created");

    let mut counter_data =
        try_from_slice_unchecked::<ReplyCounter>(&reply_counter.data.borrow()).unwrap();

    msg!("checking if counter account is already initialized");
    if counter_data.is_initialized() {
        msg!("Account already initialized");
        return Err(ProgramError::AccountAlreadyInitialized);
    }
    counter_data.discriminator = counter_discriminator.to_string();
    counter_data.counter = 0;
    counter_data.is_initialized = true;
    msg!("reply count: {}", counter_data.counter);
    counter_data.serialize(&mut &mut reply_counter.data.borrow_mut()[..])?;

    msg!("Minting 10 tokens to User associated token account");
    invoke_signed(
        // Instruction
        &mint_to(
            token_program.key,
            token_mint_pda.key,
            user_ata.key,
            mint_auth_pda.key,
            &[],
            10 * LAMPORTS_PER_SOL,
        )?, // ? unwraps and returns the error if there is one
        // Account_infos
        &[
            token_mint_pda.clone(),
            user_ata.clone(),
            mint_auth_pda.clone(),
        ],
        // Seeds
        &[&[b"token_auth", &[auth_bump]]],
    )?;

    Ok(())
}

pub fn update_student_intro(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    name: String,
    message: String,
) -> ProgramResult {
    msg!("Updating student intro...");
    msg!("Name: {}", name);
    msg!("Message: {}", message);
    let account_info_iter = &mut accounts.iter();

    let initializer = next_account_info(account_info_iter)?;
    let user_account = next_account_info(account_info_iter)?;

    msg!("unpacking state account");
    let mut account_data =
        try_from_slice_unchecked::<StudentInfo>(&user_account.data.borrow()).unwrap();
    msg!("borrowed account data");

    msg!("checking if movie account is initialized");
    if !account_data.is_initialized() {
        msg!("Account is not initialized");
        return Err(StudentIntroError::UninitializedAccount.into());
    }

    if user_account.owner != program_id {
        return Err(ProgramError::IllegalOwner);
    }

    let (pda, _bump_seed) = Pubkey::find_program_address(&[initializer.key.as_ref()], program_id);
    if pda != *user_account.key {
        msg!("Invalid seeds for PDA");
        return Err(StudentIntroError::InvalidPDA.into());
    }
    let update_len: usize = 1 + (4 + account_data.name.len()) + (4 + message.len());
    if update_len > 1000 {
        msg!("Data length is larger than 1000 bytes");
        return Err(StudentIntroError::InvalidDataLength.into());
    }

    account_data.name = account_data.name;
    account_data.msg = message;
    msg!("serializing account");
    account_data.serialize(&mut &mut user_account.data.borrow_mut()[..])?;
    msg!("state account serialized");

    Ok(())
}

pub fn add_reply(program_id: &Pubkey, accounts: &[AccountInfo], reply: String) -> ProgramResult {
    msg!("Adding Reply...");
    msg!("Reply: {}", reply);

    let account_info_iter = &mut accounts.iter();

    let replier = next_account_info(account_info_iter)?;
    let user_account = next_account_info(account_info_iter)?;
    let reply_counter = next_account_info(account_info_iter)?;
    let reply_account = next_account_info(account_info_iter)?;
    let system_program = next_account_info(account_info_iter)?;

    let mut counter_data =
        try_from_slice_unchecked::<ReplyCounter>(&reply_counter.data.borrow()).unwrap();

    let reply_discriminator = "reply";
    let account_len: usize = (4 + reply_discriminator.len()) + 1 + 32 + (4 + reply.len());

    let rent = Rent::get()?;
    let rent_lamports = rent.minimum_balance(account_len);

    let (pda, bump_seed) = Pubkey::find_program_address(
        &[
            user_account.key.as_ref(),
            counter_data.counter.to_be_bytes().as_ref(),
        ],
        program_id,
    );
    if pda != *reply_account.key {
        msg!("Invalid seeds for PDA");
        return Err(StudentIntroError::InvalidPDA.into());
    }

    invoke_signed(
        &system_instruction::create_account(
            replier.key,
            reply_account.key,
            rent_lamports,
            account_len.try_into().unwrap(),
            program_id,
        ),
        &[
            replier.clone(),
            reply_account.clone(),
            system_program.clone(),
        ],
        &[&[
            user_account.key.as_ref(),
            counter_data.counter.to_be_bytes().as_ref(),
            &[bump_seed],
        ]],
    )?;

    msg!("Created Reply Account");
    let mut reply_data = try_from_slice_unchecked::<Reply>(&reply_account.data.borrow()).unwrap();

    msg!("checking if comment account is already initialized");
    if reply_data.is_initialized() {
        msg!("Account already initialized");
        return Err(ProgramError::AccountAlreadyInitialized);
    }
    reply_data.discriminator = reply_discriminator.to_string();
    reply_data.studentinfo = *user_account.key;
    reply_data.reply = reply;
    reply_data.is_initialized = true;
    reply_data.serialize(&mut &mut reply_account.data.borrow_mut()[..])?;
    msg!("Reply Count: {}", counter_data.counter);
    counter_data.counter += 1;
    counter_data.serialize(&mut &mut reply_counter.data.borrow_mut()[..])?;
    Ok(())
}

pub fn initialize_token_mint(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();

    // The order of accounts is not arbitrary, the client will send them in this order
    // Whoever sent in the transaction
    let initializer = next_account_info(account_info_iter)?;
    // Token mint PDA - derived on the client
    let token_mint_pda = next_account_info(account_info_iter)?;
    // Token mint authorirty (this should be you)
    let mint_auth_pda = next_account_info(account_info_iter)?;
    // System program to create a new account
    let system_program = next_account_info(account_info_iter)?;
    // Solana Token program address
    let token_program = next_account_info(account_info_iter)?;
    // System account to calcuate the rent
    let sysvar_rent = next_account_info(account_info_iter)?;

    let (expected_token_mint_pda, mint_bump) =
        Pubkey::find_program_address(&[b"token_mint"], program_id);

    let (expected_mint_auth_pda, _auth_bump) =
        Pubkey::find_program_address(&[b"token_auth"], program_id);

    msg!("Token mint: {:?}", expected_token_mint_pda);
    msg!("Mint authority: {:?}", expected_mint_auth_pda);

    if expected_token_mint_pda != *token_mint_pda.key {
        msg!("Incorrect token mint account");
        return Err(StudentIntroError::IncorrectAccountError.into());
    }

    if *token_program.key != TOKEN_PROGRAM_ID {
        msg!("Incorrect token program");
        return Err(StudentIntroError::IncorrectAccountError.into());
    }

    if *mint_auth_pda.key != expected_mint_auth_pda {
        msg!("Incorrect mint auth account");
        return Err(StudentIntroError::IncorrectAccountError.into());
    }

    if *system_program.key != SYSTEM_PROGRAM_ID {
        msg!("Incorrect system program");
        return Err(StudentIntroError::IncorrectAccountError.into());
    }

    if *sysvar_rent.key != RENT_PROGRAM_ID {
        msg!("Incorrect rent program");
        return Err(StudentIntroError::IncorrectAccountError.into());
    }

    // Calculate the rent
    let rent = Rent::get()?;
    // We know the size of a mint account is 82 (remember it lol)
    let rent_lamports = rent.minimum_balance(82);

    // Create the token mint PDA
    invoke_signed(
        &system_instruction::create_account(
            initializer.key,
            token_mint_pda.key,
            rent_lamports,
            82, // Size of the token mint account
            token_program.key,
        ),
        // Accounts we're reading from or writing to
        &[
            initializer.clone(),
            token_mint_pda.clone(),
            system_program.clone(),
        ],
        // Seeds for our token mint account
        &[&[b"token_mint", &[mint_bump]]],
    )?;

    msg!("Created token mint account");

    // Initialize the mint account
    invoke_signed(
        &initialize_mint(
            token_program.key,
            token_mint_pda.key,
            mint_auth_pda.key,
            Option::None, // Freeze authority - we don't want anyone to be able to freeze!
            9,            // Number of decimals
        )?,
        // Which accounts we're reading from or writing to
        &[
            token_mint_pda.clone(),
            sysvar_rent.clone(),
            mint_auth_pda.clone(),
        ],
        // The seeds for our token mint PDA
        &[&[b"token_mint", &[mint_bump]]],
    )?;

    msg!("Initialized token mint");

    Ok(())
}
