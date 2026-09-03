import raw from './context-bootstrap-prompt.md?raw'

/**
 * A staged prompt for an external Agent to inspect Vault evidence, prepare a
 * validated Context Registry candidate, then propose Claim reassignment only
 * after the owner confirms the Registry in the trusted Memory UI.
 */
export const contextBootstrapPrompt = raw.trim()
