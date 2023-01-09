# Verus bot

This document outlines the ideas for the all-new, community developed Verus Discord bot. As you all know, the old bot made by sclear did not get any updates since sclear has gone awol. 

## V1
Version 1 just has the basics of a tipbot:

- Deposit
- Balance
- Withdraw (specific amount / all)
- Tip a user
- Tip a role
- Start a Reactdrop
- Show Verus blockchain information
- Enable or disable notifications
- Several admin commands (for Oink to be boss)


### Slash commands
The main difference with the old tipbot is that this one uses the new [Slash commands.](https://discord.com/blog/slash-commands-are-here) These are commands in which the user is guided in what commands are available and what arguments to use, since the bot developer has greater control over the arguments that need to be given to specific commands. Also, respnse messages can be made ephemeral, which means that only the user who sent the command sees the response. This will remove the need for the #tipbot channel, which greatly declutters this tipbot channel.

### Development
This bot was made in Rust, using the Poise + Serenity framework for Discord bots. The code repository lives at [Github](https://github.com/verus-discord-bot/bot).

## V2 and beyond

(This list is a collection of ideas, none of which are guaranteed to be implemented)

- PBaaS currency support (tip someone a currency e.g. vETH)
- Signature checking
- VerusID profile display
- VerusID lock / unlock notifications (subscription based)
- Use VerusID to log in and get additional features in the bot
- Marketplace support (see open offers)
- Advanced Verus price information (denominated in BTC and USD)
- Private transactions