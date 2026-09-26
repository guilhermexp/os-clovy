import { defineMessages } from "../define";

// Copy produced by non-React helpers in src/lib (option labels, notification
// text, error messages, schedule descriptions). Strings sent to the model or
// agent as instructions are not UI and stay in English in their source files.
export default defineMessages({
  en: {
    // Dictation / transcription language names (the value codes never change).
    "lib.dictationLanguage.auto": "Auto-detect",
    "lib.dictationLanguage.en": "English",
    "lib.dictationLanguage.ar": "Arabic",
    "lib.dictationLanguage.zh": "Chinese",
    "lib.dictationLanguage.nl": "Dutch",
    "lib.dictationLanguage.fr": "French",
    "lib.dictationLanguage.de": "German",
    "lib.dictationLanguage.hi": "Hindi",
    "lib.dictationLanguage.id": "Indonesian",
    "lib.dictationLanguage.it": "Italian",
    "lib.dictationLanguage.ja": "Japanese",
    "lib.dictationLanguage.ko": "Korean",
    "lib.dictationLanguage.no": "Norwegian",
    "lib.dictationLanguage.pl": "Polish",
    "lib.dictationLanguage.pt": "Portuguese",
    "lib.dictationLanguage.ru": "Russian",
    "lib.dictationLanguage.es": "Spanish",
    "lib.dictationLanguage.sv": "Swedish",
    "lib.dictationLanguage.th": "Thai",
    "lib.dictationLanguage.tr": "Turkish",
    "lib.dictationLanguage.uk": "Ukrainian",
    "lib.dictationLanguage.vi": "Vietnamese",

    // Text size presets.
    "lib.fontScale.default": "Default",
    "lib.fontScale.large": "Large",
    "lib.fontScale.larger": "Larger",

    // Accent color presets.
    "lib.brand.sage": "Sage",
    "lib.brand.clay": "Clay",
    "lib.brand.rose": "Rose",
    "lib.brand.ocean": "Ocean",
    "lib.brand.plum": "Plum",

    // Errors.
    "lib.errors.sharingUnavailable":
      "Sharing isn't available on this Clovy server yet. Try again after the next update.",
    "lib.errors.shareNotFound": "This share no longer exists. It may have been stopped.",
    "lib.errors.timedOut": "Operation timed out.",
    "lib.errors.accountStatusTimeout": "Account status took too long. Please try again.",
    "lib.errors.fileDropUnreadable": "Could not read the dropped file.",
    "lib.errors.fileDropEmpty": "Drop files from Finder to attach them to Clovy.",
    "lib.errors.fileDropTooMany": {
      one: "You can attach up to {count} file at a time.",
      other: "You can attach up to {count} files at a time.",
    },
    "lib.errors.fileDropTooLarge": "Dropped files must be 50 MB or smaller.",
    "lib.errors.fileDropFolder": 'Could not read "{name}". Folders can\'t be attached.',
    "lib.errors.fileDropThisItem": "this item",
    "lib.errors.imagePromptRequired": "Enter a prompt to generate an image.",
    "lib.errors.imageUndisplayable": "Clovy returned an image it can't display.",
    "lib.errors.imageEditInstructionRequired": "Enter an edit instruction.",
    "lib.errors.imageSourceUnreadable": "Clovy couldn't read the source image.",
    "lib.errors.videoPromptRequired": "Enter a prompt to generate a video.",
    "lib.errors.videoStillRunning": "Video generation is still running. Try again later.",
    "lib.errors.slashFileParse": "Could not parse /file paths. Close the quote and try again.",

    // Skill slash command errors.
    "lib.skillSlash.ambiguous": "/{token} matches more than one skill. Use {matches}.",
    "lib.skillSlash.disabledToken": "/{token} is disabled. Enable it in Agent settings to use it.",
    "lib.skillSlash.disabledOne": "{matches} is disabled. Enable it in Agent settings to use it.",
    "lib.skillSlash.disabledMany":
      "{matches} are disabled. Enable one in Agent settings to use it.",
    "lib.skillSlash.missingWithSuggestions": "Could not find skill /{token}. Try {suggestions}.",
    "lib.skillSlash.missing": "Could not find skill /{token}.",

    // Built-in composer slash commands.
    "lib.slash.model.label": "Model",
    "lib.slash.model.description": "Change the text model.",
    "lib.slash.file.label": "File",
    "lib.slash.file.description": "Attach files to this message.",
    "lib.slash.image.label": "Image",
    "lib.slash.image.description": "Generate an image from a prompt.",
    "lib.slash.video.label": "Video",
    "lib.slash.video.description": "Generate a video from a prompt.",

    // Agent tool activity labels.
    "lib.toolActivity.computerUse": "Computer use",
    "lib.toolActivity.usingTool": "Using a tool.",
    "lib.toolActivity.sentence": "{label}.",
    "lib.toolActivity.tool": "Tool",
    "lib.toolActivity.browsing": "Browsing",
    "lib.toolActivity.searchingWeb": "Searching web",
    "lib.toolActivity.searchingImages": "Searching images",
    "lib.toolActivity.searchingFiles": "Searching files",
    "lib.toolActivity.searching": "Searching",
    "lib.toolActivity.editingFiles": "Editing files",
    "lib.toolActivity.readingFiles": "Reading files",
    "lib.toolActivity.usingGitHub": "Using GitHub",
    "lib.toolActivity.inspectingRepository": "Inspecting repository",
    "lib.toolActivity.runningTests": "Running tests",
    "lib.toolActivity.building": "Building",
    "lib.toolActivity.checkingCode": "Checking code",
    "lib.toolActivity.runningCommand": "Running command",
    "lib.toolActivity.workingWithVideo": "Working with video",
    "lib.toolActivity.workingWithImages": "Working with images",

    // Agent native notifications.
    "lib.agentNotify.ready": "Clovy is ready",
    "lib.agentNotify.needsInput": "Clovy needs your input",
    "lib.agentNotify.finished": "Clovy finished",
    "lib.agentNotify.stopped": "Clovy stopped",
    "lib.agentNotify.problem": "Clovy hit a problem",
    "lib.agentNotify.fallbackSubject": "Agent session",

    // Recording native notifications.
    "lib.recordingNotify.stillInMeetingTitle": "Still in a meeting?",
    "lib.recordingNotify.stillInMeetingBody": {
      one: "Clovy will pause the recording in {count} second if you do not answer.",
      other: "Clovy will pause the recording in {count} seconds if you do not answer.",
    },
    "lib.recordingNotify.autoPausedTitle": "Clovy paused recording",
    "lib.recordingNotify.autoPausedBody":
      "No meeting audio was detected. Open Clovy to resume or finish.",

    // Routine schedule descriptions.
    "lib.schedule.everyMinute": "Every minute",
    "lib.schedule.everyNMinutes": {
      one: "Every {count} minute",
      other: "Every {count} minutes",
    },
    "lib.schedule.everyHour": "Every hour",
    "lib.schedule.everyHourAtMinute": "Every hour at :{minute}",
    "lib.schedule.everyNHours": { one: "Every {count} hour", other: "Every {count} hours" },
    "lib.schedule.everyDayAt": "Every day at {time}",
    "lib.schedule.weekdaysAt": "Weekdays at {time}",
    "lib.schedule.weekendsAt": "Weekends at {time}",
    "lib.schedule.everyWeekdayAt": "Every {day} at {time}",
    "lib.schedule.everyWeekendDayAt": "Every {day} at {time}",
    "lib.schedule.dayRangeAt": "Every {from} to {to} at {time}",
    "lib.schedule.daysAt": "Every {days} at {time}",
    "lib.schedule.monthlyAt": {
      one: "Monthly on the {days} at {time}",
      other: "Monthly on the {days} at {time}",
    },
    "lib.schedule.yearlyAt": "Every year on {dates} at {time}",

    // Account gate.
    "lib.accountGate.topUp": "Top up credits",
    "lib.accountGate.upgradeToMax": "Upgrade to Max",
    "lib.accountGate.upgrade": "Upgrade",

    // Max upgrade copy.
    "lib.maxUpgrade.confirmTitle": "Upgrade to Max?",
    "lib.maxUpgrade.confirmBody":
      "Max is $100 per month. A secure Stripe page will open in your browser to review and confirm. Your billing cycle restarts today.",
    "lib.maxUpgrade.chargeConfirmBody":
      "Max is $100 per month, charged to your saved card now. Your billing cycle restarts today.",
    "lib.maxUpgrade.confirmLabel": "Upgrade now",
    "lib.maxUpgrade.busyLabel": "Upgrading...",
    "lib.maxUpgrade.browserStatus": "Waiting for you to confirm in the browser",
    "lib.maxUpgrade.waitingStatus": "Upgrade started. Waiting for payment confirmation.",
    "lib.maxUpgrade.readyStatus": "Max is active.",
    "lib.maxUpgrade.slowStatus": "Payment not confirmed yet. Check billing in your account portal.",
    "lib.maxUpgrade.hostedSlowStatus":
      "Still waiting for payment confirmation. If you closed the Stripe page, you can try again.",
    "lib.maxUpgrade.portalLabel": "Open billing",
    "lib.maxUpgrade.staleActionNotice": "Your plan changed - pick an option again",

    // Upstream provider recovery and notices.
    "lib.upstream.tryAgain": "Try again",
    "lib.upstream.noticeBody": "The upstream provider could not complete this request.",
    "lib.companion.messageTruncated": "[Message truncated on companion]",
    "lib.companion.autoDescription": "Chooses the best available model for each request.",

    // Routines.
    "lib.routines.legacyPaused":
      "This imported routine uses legacy execution settings. Review and recreate it before running.",

    // Connectors: scope bundles.
    "lib.connectors.gmailRead.label": "Read mail",
    "lib.connectors.gmailRead.description": "Search and read your email for briefings and triage.",
    "lib.connectors.gmailRead.feature": "read your mail",
    "lib.connectors.gmailDraft.label": "Draft replies",
    "lib.connectors.gmailDraft.description": "Write draft replies for you to review. Never sends.",
    "lib.connectors.gmailDraft.feature": "draft replies",
    "lib.connectors.gmailModify.label": "Organize mail",
    "lib.connectors.gmailModify.description": "Label and archive your mail. Never deletes.",
    "lib.connectors.gmailModify.feature": "label and archive mail",
    "lib.connectors.gmailSend.label": "Send mail",
    "lib.connectors.gmailSend.description":
      "Send email on your behalf. Only used when you allow it per routine.",
    "lib.connectors.gmailSend.feature": "send mail",
    "lib.connectors.calendarRead.label": "Read calendar",
    "lib.connectors.calendarRead.description":
      "Read your events and find free slots for briefings and prep.",
    "lib.connectors.calendarRead.feature": "read your calendar",
    "lib.connectors.calendarEvents.label": "Manage calendar",
    "lib.connectors.calendarEvents.description":
      "Create events and respond to invites on your behalf.",
    "lib.connectors.calendarEvents.feature": "manage your calendar",
    "lib.connectors.linearRead.label": "Read workspace",
    "lib.connectors.linearRead.description":
      "Read teams, projects, cycles, and issues for planning and status briefs.",
    "lib.connectors.linearRead.feature": "read your Linear workspace",
    "lib.connectors.linearWrite.label": "Create and update issues",
    "lib.connectors.linearWrite.description":
      "Draft issues, comments, and project updates. Nothing is written without your approval.",
    "lib.connectors.linearWrite.feature": "create and update issues",
    "lib.connectors.githubRead.label": "Read repositories, issues, and pull requests",
    "lib.connectors.githubRead.description":
      "Read code, issues, pull requests, and comments in the repositories chosen during GitHub App installation.",
    "lib.connectors.githubRead.feature": "read your GitHub repositories",
    "lib.connectors.githubWrite.label": "Create and update issues and comments",
    "lib.connectors.githubWrite.description":
      "Clovy allows drafting issues and comments on your behalf. Every write asks for your approval before it runs.",
    "lib.connectors.githubWrite.feature": "create and update issues",
    "lib.connectors.bundleFallbackDescription": "Connector capability.",

    // Connectors: account status.
    "lib.connectors.status.connected": "Connected",
    "lib.connectors.status.reconnectRequired": "Reconnect needed",
    "lib.connectors.status.unavailable": "Status unavailable",
    "lib.connectors.blurb.connected": "This account is ready. Tokens stay in your Mac's Keychain.",
    "lib.connectors.blurb.reconnectGoogle":
      "Google needs you to sign in again before Clovy can use this account.",
    "lib.connectors.blurb.reconnectLinear":
      "Linear needs you to sign in again before Clovy can use this workspace.",
    "lib.connectors.blurb.reconnectNotion":
      "Notion needs you to connect again before Clovy can use its hosted MCP tools.",
    "lib.connectors.blurb.reconnectGithub":
      "GitHub needs you to sign in again before Clovy can use this account.",
    "lib.connectors.blurb.unavailable":
      "Clovy could not confirm the Notion connection. Try again in a moment.",

    // Connectors: trust modes and earned autonomy.
    "lib.connectors.trust.readOnly.label": "Read only",
    "lib.connectors.trust.readOnly.description":
      "The routine can read mail and calendar but never change anything.",
    "lib.connectors.trust.approval.label": "Approval",
    "lib.connectors.trust.approval.description":
      "Drafts, sends, and event changes wait for your approval before they run.",
    "lib.connectors.trust.autonomous.label": "Autonomous",
    "lib.connectors.trust.autonomous.description":
      "Tools you grant run without asking. Unlocked after a few runs under approval.",
    "lib.connectors.autonomyUnlocked": "Autonomous is unlocked for this routine.",
    "lib.connectors.autonomyUnlockHint": {
      one: "Runs {count} more time under approval to unlock autonomous.",
      other: "Runs {count} more times under approval to unlock autonomous.",
    },
    "lib.connectors.autonomyProgressDone": "Autonomy unlocked.",
    "lib.connectors.autonomyProgress":
      "Run {next} of {threshold} under approval before autonomy unlocks.",

    // Connectors: action tools.
    "lib.connectors.action.gmailCreateDraft": "Create drafts",
    "lib.connectors.action.gmailSendEmail": "Send email",
    "lib.connectors.action.gmailModifyLabels": "Change labels",
    "lib.connectors.action.gmailArchive": "Archive mail",
    "lib.connectors.action.gcalCreateEvent": "Create events",
    "lib.connectors.action.gcalRespondToInvite": "Respond to invites",
    "lib.connectors.action.linearCreateIssue": "Create issues",
    "lib.connectors.action.linearUpdateIssue": "Update issues",
    "lib.connectors.action.linearAddComment": "Comment on issues",
    "lib.connectors.action.linearCreateProjectUpdate": "Post project updates",
    "lib.connectors.action.notionCreatePages": "Create Notion pages",
    "lib.connectors.action.notionUpdatePage": "Update Notion pages",
    "lib.connectors.action.githubCreateIssue": "Create issue",
    "lib.connectors.action.githubUpdateIssue": "Update issue",
    "lib.connectors.action.githubAddComment": "Add comment",

    // Connectors: event triggers.
    "lib.connectors.trigger.emailReceived.label": "When new email arrives",
    "lib.connectors.trigger.emailReceived.description":
      "Runs when new mail lands in the connected inbox.",
    "lib.connectors.trigger.eventUpcoming.label": "Before an upcoming meeting",
    "lib.connectors.trigger.eventUpcoming.description":
      "Runs a set number of minutes before a calendar event starts.",
    "lib.connectors.triggerScopeWarning":
      "This trigger needs {features} access on your connected Google account. Add it in Settings under Plugins.",

    // Model privacy.
    "lib.modelPrivacy.e2eeLabel": "E2EE",
    "lib.modelPrivacy.privateLabel": "Private mode",
    "lib.modelPrivacy.anonymousLabel": "Anonymous mode",
    "lib.modelPrivacy.e2eeDescription":
      "Private model with end-to-end encryption. Your prompt is encrypted on your device and only decrypted inside a hardware-secured enclave (TEE); the response is encrypted before it leaves the enclave. No prompt data is ever readable by the model provider or its infrastructure.",
    "lib.modelPrivacy.privateDescription":
      "Private model with zero data retention. No prompt data is stored, shared with a third party, or trained on.",
    "lib.modelPrivacy.anonymousDescription":
      "The model provider may retain prompts, though they're anonymized. Your identity is stripped before anything leaves Clovy. For sensitive content, pick a Private or E2EE model.",

    // Model pricing.
    "lib.modelPricing.inOut": "{input} in / {output} out",
    "lib.modelPricing.perSecondAudio": "{price} per second audio",
    "lib.modelPricing.perMillionTokens": "{input} input / {output} output per 1M tokens",

    // Local models.
    "lib.localGeneration.unavailableDescription": "This local model is no longer configured.",

    // Thinking level.
    "lib.thinking.low.label": "Low",
    "lib.thinking.low.blurb": "Faster responses with lower usage.",
    "lib.thinking.medium.label": "Medium",
    "lib.thinking.medium.blurb": "Balances speed and depth for most tasks.",
    "lib.thinking.high.label": "High",
    "lib.thinking.high.blurb": "Deeper reasoning with higher usage.",

    // Suggested models.
    "lib.suggested.autoEconomy": "Economy",
    "lib.suggested.autoQuality": "Quality",
    "lib.suggested.autoBalanced": "Balanced",
    "lib.suggested.glm52":
      "Default pick: latest GLM flagship with strong reasoning, tool use, structured output, and zero data retention.",
    "lib.suggested.kimiK3":
      "Newest Kimi flagship: multimodal reasoning, tool use, and a 1M-token context window in anonymous mode.",
    "lib.suggested.kimiK26":
      "Private Kimi option: multimodal reasoning, tool use, and a 256K-token context window with zero data retention.",
    "lib.suggested.glm51":
      "Stable GLM alternate: previous GLM flagship with top-tier agentic coding, tool use, and zero data retention.",
    "lib.suggested.parakeet":
      "Fast and accurate for everyday dictation and meetings, zero data retention, lowest price tier.",
    "lib.suggested.whisper":
      "Best multilingual accuracy at the same low price, with zero data retention.",
    "lib.suggested.veniceSd35":
      "Default pick: Venice's Stable Diffusion 3.5 model, a private all-rounder for everyday images.",
    "lib.suggested.zImageTurbo":
      "Fastest pick: quick, low-cost generations with zero data retention.",
    "lib.suggested.qwenImage":
      "Quality pick: strong text rendering and prompt adherence for detailed images.",
    "lib.suggested.lustifyV8":
      "Uncensored pick: the least restricted image model, with zero data retention.",
    "lib.suggested.wan22": "Default pick: fast 5 second 720p clips, the lowest-cost option.",
    "lib.suggested.grokImagineVideo":
      "Photorealistic pick: lifelike clips with audio, zero data retention.",
    "lib.suggested.ltx2": "Quality pick: higher-detail open-source model with audio.",

    // Image and video model descriptions.
    "lib.imageModels.veniceSd35": "Venice's default Stable Diffusion 3.5 image model.",
    "lib.imageModels.flux2Pro": "High-detail FLUX model for photorealistic results.",
    "lib.imageModels.qwenImage": "Strong text rendering and prompt adherence.",
    "lib.imageModels.chroma": "Versatile general-purpose image model.",
    "lib.videoModels.wan22": "Default text-to-video model for fast 5 second 720p clips.",
    "lib.videoModels.grokImagine": "Photorealistic clips with audio.",
    "lib.videoModels.ltx2": "Higher-detail open-source model with audio.",

    // Home.
    "lib.home.message": "Home message",
    "lib.home.todayAt": "Today at {time}",
    "lib.home.yesterdayAt": "Yesterday at {time}",
    "lib.home.dayAt": "{day} at {time}",
    "lib.home.greetingWithName": "{salutation}, {name}",
    "lib.home.goodMorning": "Good morning",
    "lib.home.goodAfternoon": "Good afternoon",
    "lib.home.goodEvening": "Good evening",
    "lib.home.pickUpToday": "What should we pick up today?",
    "lib.home.helpToday": "What would you like help with today?",
    "lib.home.pickUpAfternoon": "What should we pick up this afternoon?",
    "lib.home.helpAfternoon": "What would you like help with this afternoon?",
    "lib.home.pickUpEvening": "What should we pick up this evening?",
    "lib.home.helpEvening": "What would you like help with this evening?",
    "lib.home.nudge.planMyDay": "Plan my day",
    "lib.home.nudge.thinkThroughDecision": "Think through a decision",
    "lib.home.nudge.getSomethingDone": "Help me get something done",
    "lib.home.nudge.planRestOfDay": "Plan the rest of my day",
    "lib.home.nudge.workThroughBlocker": "Work through a blocker",
    "lib.home.nudge.prioritize": "Help me prioritize",
    "lib.home.nudge.reviewMyDay": "Review my day",
    "lib.home.nudge.planTomorrow": "Plan tomorrow",
    "lib.home.checkIn": "{salutation}. {question}",

    // Note PDF export.
    "lib.notePdf.defaultTitle": "Meeting notes",
  },
  "pt-BR": {
    "lib.dictationLanguage.auto": "Detectar automaticamente",
    "lib.dictationLanguage.en": "Inglês",
    "lib.dictationLanguage.ar": "Árabe",
    "lib.dictationLanguage.zh": "Chinês",
    "lib.dictationLanguage.nl": "Holandês",
    "lib.dictationLanguage.fr": "Francês",
    "lib.dictationLanguage.de": "Alemão",
    "lib.dictationLanguage.hi": "Hindi",
    "lib.dictationLanguage.id": "Indonésio",
    "lib.dictationLanguage.it": "Italiano",
    "lib.dictationLanguage.ja": "Japonês",
    "lib.dictationLanguage.ko": "Coreano",
    "lib.dictationLanguage.no": "Norueguês",
    "lib.dictationLanguage.pl": "Polonês",
    "lib.dictationLanguage.pt": "Português",
    "lib.dictationLanguage.ru": "Russo",
    "lib.dictationLanguage.es": "Espanhol",
    "lib.dictationLanguage.sv": "Sueco",
    "lib.dictationLanguage.th": "Tailandês",
    "lib.dictationLanguage.tr": "Turco",
    "lib.dictationLanguage.uk": "Ucraniano",
    "lib.dictationLanguage.vi": "Vietnamita",

    "lib.fontScale.default": "Padrão",
    "lib.fontScale.large": "Grande",
    "lib.fontScale.larger": "Maior",

    "lib.brand.sage": "Sálvia",
    "lib.brand.clay": "Argila",
    "lib.brand.rose": "Rosa",
    "lib.brand.ocean": "Oceano",
    "lib.brand.plum": "Ameixa",

    "lib.errors.sharingUnavailable":
      "O compartilhamento ainda não está disponível neste servidor do Clovy. Tente de novo após a próxima atualização.",
    "lib.errors.shareNotFound":
      "Este compartilhamento não existe mais. Ele pode ter sido interrompido.",
    "lib.errors.timedOut": "A operação expirou.",
    "lib.errors.accountStatusTimeout": "O status da conta demorou demais. Tente de novo.",
    "lib.errors.fileDropUnreadable": "Não foi possível ler o arquivo arrastado.",
    "lib.errors.fileDropEmpty": "Solte arquivos do Finder para anexá-los ao Clovy.",
    "lib.errors.fileDropTooMany": {
      one: "Você pode anexar até {count} arquivo por vez.",
      other: "Você pode anexar até {count} arquivos por vez.",
    },
    "lib.errors.fileDropTooLarge": "Os arquivos arrastados devem ter 50 MB ou menos.",
    "lib.errors.fileDropFolder": 'Não foi possível ler "{name}". Não é possível anexar pastas.',
    "lib.errors.fileDropThisItem": "este item",
    "lib.errors.imagePromptRequired": "Digite um prompt para gerar uma imagem.",
    "lib.errors.imageUndisplayable": "O Clovy retornou uma imagem que não consegue exibir.",
    "lib.errors.imageEditInstructionRequired": "Digite uma instrução de edição.",
    "lib.errors.imageSourceUnreadable": "O Clovy não conseguiu ler a imagem de origem.",
    "lib.errors.videoPromptRequired": "Digite um prompt para gerar um vídeo.",
    "lib.errors.videoStillRunning":
      "A geração do vídeo ainda está em andamento. Tente de novo mais tarde.",
    "lib.errors.slashFileParse":
      "Não foi possível interpretar os caminhos do /file. Feche as aspas e tente de novo.",

    "lib.skillSlash.ambiguous": "/{token} corresponde a mais de uma skill. Use {matches}.",
    "lib.skillSlash.disabledToken":
      "/{token} está desativada. Ative-a nas configurações do Agente para usá-la.",
    "lib.skillSlash.disabledOne":
      "{matches} está desativada. Ative-a nas configurações do Agente para usá-la.",
    "lib.skillSlash.disabledMany":
      "{matches} estão desativadas. Ative uma delas nas configurações do Agente para usá-la.",
    "lib.skillSlash.missingWithSuggestions":
      "Não foi possível encontrar a skill /{token}. Tente {suggestions}.",
    "lib.skillSlash.missing": "Não foi possível encontrar a skill /{token}.",

    "lib.slash.model.label": "Modelo",
    "lib.slash.model.description": "Trocar o modelo de texto.",
    "lib.slash.file.label": "Arquivo",
    "lib.slash.file.description": "Anexar arquivos a esta mensagem.",
    "lib.slash.image.label": "Imagem",
    "lib.slash.image.description": "Gerar uma imagem a partir de um prompt.",
    "lib.slash.video.label": "Vídeo",
    "lib.slash.video.description": "Gerar um vídeo a partir de um prompt.",

    "lib.toolActivity.computerUse": "Uso do computador",
    "lib.toolActivity.usingTool": "Usando uma ferramenta.",
    "lib.toolActivity.sentence": "{label}.",
    "lib.toolActivity.tool": "Ferramenta",
    "lib.toolActivity.browsing": "Navegando",
    "lib.toolActivity.searchingWeb": "Buscando na web",
    "lib.toolActivity.searchingImages": "Buscando imagens",
    "lib.toolActivity.searchingFiles": "Buscando arquivos",
    "lib.toolActivity.searching": "Buscando",
    "lib.toolActivity.editingFiles": "Editando arquivos",
    "lib.toolActivity.readingFiles": "Lendo arquivos",
    "lib.toolActivity.usingGitHub": "Usando o GitHub",
    "lib.toolActivity.inspectingRepository": "Inspecionando o repositório",
    "lib.toolActivity.runningTests": "Executando testes",
    "lib.toolActivity.building": "Compilando",
    "lib.toolActivity.checkingCode": "Verificando o código",
    "lib.toolActivity.runningCommand": "Executando comando",
    "lib.toolActivity.workingWithVideo": "Trabalhando com vídeo",
    "lib.toolActivity.workingWithImages": "Trabalhando com imagens",

    "lib.agentNotify.ready": "O Clovy está pronto",
    "lib.agentNotify.needsInput": "O Clovy precisa da sua resposta",
    "lib.agentNotify.finished": "O Clovy terminou",
    "lib.agentNotify.stopped": "O Clovy parou",
    "lib.agentNotify.problem": "O Clovy encontrou um problema",
    "lib.agentNotify.fallbackSubject": "Sessão do agente",

    "lib.recordingNotify.stillInMeetingTitle": "Ainda em reunião?",
    "lib.recordingNotify.stillInMeetingBody": {
      one: "O Clovy vai pausar a gravação em {count} segundo se você não responder.",
      other: "O Clovy vai pausar a gravação em {count} segundos se você não responder.",
    },
    "lib.recordingNotify.autoPausedTitle": "O Clovy pausou a gravação",
    "lib.recordingNotify.autoPausedBody":
      "Nenhum áudio de reunião foi detectado. Abra o Clovy para retomar ou finalizar.",

    "lib.schedule.everyMinute": "A cada minuto",
    "lib.schedule.everyNMinutes": {
      one: "A cada {count} minuto",
      other: "A cada {count} minutos",
    },
    "lib.schedule.everyHour": "A cada hora",
    "lib.schedule.everyHourAtMinute": "A cada hora, no minuto {minute}",
    "lib.schedule.everyNHours": { one: "A cada {count} hora", other: "A cada {count} horas" },
    "lib.schedule.everyDayAt": "Todo dia às {time}",
    "lib.schedule.weekdaysAt": "Todo dia útil às {time}",
    "lib.schedule.weekendsAt": "Todo fim de semana às {time}",
    "lib.schedule.everyWeekdayAt": "Toda {day} às {time}",
    "lib.schedule.everyWeekendDayAt": "Todo {day} às {time}",
    "lib.schedule.dayRangeAt": "De {from} a {to} às {time}",
    "lib.schedule.daysAt": "Toda semana: {days} às {time}",
    "lib.schedule.monthlyAt": {
      one: "Todo mês no dia {days} às {time}",
      other: "Todo mês nos dias {days} às {time}",
    },
    "lib.schedule.yearlyAt": "Todo ano em {dates} às {time}",

    "lib.accountGate.topUp": "Recarregar créditos",
    "lib.accountGate.upgradeToMax": "Fazer upgrade para o Max",
    "lib.accountGate.upgrade": "Fazer upgrade",

    "lib.maxUpgrade.confirmTitle": "Fazer upgrade para o Max?",
    "lib.maxUpgrade.confirmBody":
      "O Max custa US$ 100 por mês. Uma página segura do Stripe vai abrir no seu navegador para você revisar e confirmar. Seu ciclo de cobrança recomeça hoje.",
    "lib.maxUpgrade.chargeConfirmBody":
      "O Max custa US$ 100 por mês, cobrados agora no seu cartão salvo. Seu ciclo de cobrança recomeça hoje.",
    "lib.maxUpgrade.confirmLabel": "Fazer upgrade agora",
    "lib.maxUpgrade.busyLabel": "Fazendo upgrade...",
    "lib.maxUpgrade.browserStatus": "Aguardando sua confirmação no navegador",
    "lib.maxUpgrade.waitingStatus": "Upgrade iniciado. Aguardando a confirmação do pagamento.",
    "lib.maxUpgrade.readyStatus": "O Max está ativo.",
    "lib.maxUpgrade.slowStatus":
      "Pagamento ainda não confirmado. Verifique a cobrança no portal da sua conta.",
    "lib.maxUpgrade.hostedSlowStatus":
      "Ainda aguardando a confirmação do pagamento. Se você fechou a página do Stripe, pode tentar de novo.",
    "lib.maxUpgrade.portalLabel": "Abrir cobrança",
    "lib.maxUpgrade.staleActionNotice": "Seu plano mudou - escolha uma opção de novo",

    "lib.upstream.tryAgain": "Tentar de novo",
    "lib.upstream.noticeBody": "O provedor do modelo não conseguiu concluir esta solicitação.",
    "lib.companion.messageTruncated": "[Mensagem truncada no dispositivo vinculado]",
    "lib.companion.autoDescription": "Escolhe o melhor modelo disponível para cada solicitação.",

    "lib.routines.legacyPaused":
      "Esta rotina importada usa configurações de execução antigas. Revise e recrie a rotina antes de executá-la.",

    "lib.connectors.gmailRead.label": "Ler e-mails",
    "lib.connectors.gmailRead.description": "Buscar e ler seus e-mails para resumos e triagem.",
    "lib.connectors.gmailRead.feature": "ler seus e-mails",
    "lib.connectors.gmailDraft.label": "Rascunhar respostas",
    "lib.connectors.gmailDraft.description":
      "Escrever rascunhos de respostas para você revisar. Nunca envia.",
    "lib.connectors.gmailDraft.feature": "rascunhar respostas",
    "lib.connectors.gmailModify.label": "Organizar e-mails",
    "lib.connectors.gmailModify.description":
      "Aplicar marcadores e arquivar seus e-mails. Nunca exclui.",
    "lib.connectors.gmailModify.feature": "aplicar marcadores e arquivar e-mails",
    "lib.connectors.gmailSend.label": "Enviar e-mails",
    "lib.connectors.gmailSend.description":
      "Enviar e-mails em seu nome. Usado apenas quando você permite em cada rotina.",
    "lib.connectors.gmailSend.feature": "enviar e-mails",
    "lib.connectors.calendarRead.label": "Ler agenda",
    "lib.connectors.calendarRead.description":
      "Ler seus eventos e encontrar horários livres para resumos e preparação.",
    "lib.connectors.calendarRead.feature": "ler sua agenda",
    "lib.connectors.calendarEvents.label": "Gerenciar agenda",
    "lib.connectors.calendarEvents.description":
      "Criar eventos e responder a convites em seu nome.",
    "lib.connectors.calendarEvents.feature": "gerenciar sua agenda",
    "lib.connectors.linearRead.label": "Ler workspace",
    "lib.connectors.linearRead.description":
      "Ler equipes, projetos, ciclos e issues para planejamento e resumos de status.",
    "lib.connectors.linearRead.feature": "ler seu workspace do Linear",
    "lib.connectors.linearWrite.label": "Criar e atualizar issues",
    "lib.connectors.linearWrite.description":
      "Rascunhar issues, comentários e atualizações de projeto. Nada é gravado sem a sua aprovação.",
    "lib.connectors.linearWrite.feature": "criar e atualizar issues",
    "lib.connectors.githubRead.label": "Ler repositórios, issues e pull requests",
    "lib.connectors.githubRead.description":
      "Ler código, issues, pull requests e comentários nos repositórios escolhidos durante a instalação do GitHub App.",
    "lib.connectors.githubRead.feature": "ler seus repositórios do GitHub",
    "lib.connectors.githubWrite.label": "Criar e atualizar issues e comentários",
    "lib.connectors.githubWrite.description":
      "O Clovy pode rascunhar issues e comentários em seu nome. Cada gravação pede a sua aprovação antes de ser executada.",
    "lib.connectors.githubWrite.feature": "criar e atualizar issues",
    "lib.connectors.bundleFallbackDescription": "Recurso do conector.",

    "lib.connectors.status.connected": "Conectado",
    "lib.connectors.status.reconnectRequired": "Reconexão necessária",
    "lib.connectors.status.unavailable": "Status indisponível",
    "lib.connectors.blurb.connected":
      "Esta conta está pronta. Os tokens ficam nas Chaves do seu Mac.",
    "lib.connectors.blurb.reconnectGoogle":
      "O Google precisa que você entre de novo antes que o Clovy possa usar esta conta.",
    "lib.connectors.blurb.reconnectLinear":
      "O Linear precisa que você entre de novo antes que o Clovy possa usar este workspace.",
    "lib.connectors.blurb.reconnectNotion":
      "O Notion precisa que você conecte de novo antes que o Clovy possa usar as ferramentas MCP hospedadas.",
    "lib.connectors.blurb.reconnectGithub":
      "O GitHub precisa que você entre de novo antes que o Clovy possa usar esta conta.",
    "lib.connectors.blurb.unavailable":
      "O Clovy não conseguiu confirmar a conexão com o Notion. Tente de novo em instantes.",

    "lib.connectors.trust.readOnly.label": "Somente leitura",
    "lib.connectors.trust.readOnly.description":
      "A rotina pode ler e-mails e a agenda, mas nunca altera nada.",
    "lib.connectors.trust.approval.label": "Aprovação",
    "lib.connectors.trust.approval.description":
      "Rascunhos, envios e alterações de eventos aguardam a sua aprovação antes de serem executados.",
    "lib.connectors.trust.autonomous.label": "Autônomo",
    "lib.connectors.trust.autonomous.description":
      "As ferramentas que você autoriza são executadas sem perguntar. Liberado após algumas execuções com aprovação.",
    "lib.connectors.autonomyUnlocked": "O modo autônomo está liberado para esta rotina.",
    "lib.connectors.autonomyUnlockHint": {
      one: "Execute mais {count} vez com aprovação para liberar o modo autônomo.",
      other: "Execute mais {count} vezes com aprovação para liberar o modo autônomo.",
    },
    "lib.connectors.autonomyProgressDone": "Autonomia liberada.",
    "lib.connectors.autonomyProgress":
      "Execução {next} de {threshold} com aprovação antes de liberar a autonomia.",

    "lib.connectors.action.gmailCreateDraft": "Criar rascunhos",
    "lib.connectors.action.gmailSendEmail": "Enviar e-mail",
    "lib.connectors.action.gmailModifyLabels": "Alterar marcadores",
    "lib.connectors.action.gmailArchive": "Arquivar e-mails",
    "lib.connectors.action.gcalCreateEvent": "Criar eventos",
    "lib.connectors.action.gcalRespondToInvite": "Responder a convites",
    "lib.connectors.action.linearCreateIssue": "Criar issues",
    "lib.connectors.action.linearUpdateIssue": "Atualizar issues",
    "lib.connectors.action.linearAddComment": "Comentar em issues",
    "lib.connectors.action.linearCreateProjectUpdate": "Publicar atualizações de projeto",
    "lib.connectors.action.notionCreatePages": "Criar páginas no Notion",
    "lib.connectors.action.notionUpdatePage": "Atualizar páginas no Notion",
    "lib.connectors.action.githubCreateIssue": "Criar issue",
    "lib.connectors.action.githubUpdateIssue": "Atualizar issue",
    "lib.connectors.action.githubAddComment": "Adicionar comentário",

    "lib.connectors.trigger.emailReceived.label": "Quando chegar um novo e-mail",
    "lib.connectors.trigger.emailReceived.description":
      "É executada quando um novo e-mail chega na caixa de entrada conectada.",
    "lib.connectors.trigger.eventUpcoming.label": "Antes de uma reunião",
    "lib.connectors.trigger.eventUpcoming.description":
      "É executada alguns minutos antes do início de um evento da agenda.",
    "lib.connectors.triggerScopeWarning":
      "Este gatilho precisa de acesso para {features} na sua conta Google conectada. Adicione em Configurações, em Plugins.",

    "lib.modelPrivacy.e2eeLabel": "E2EE",
    "lib.modelPrivacy.privateLabel": "Modo privado",
    "lib.modelPrivacy.anonymousLabel": "Modo anônimo",
    "lib.modelPrivacy.e2eeDescription":
      "Modelo privado com criptografia de ponta a ponta. Seu prompt é criptografado no seu dispositivo e só é descriptografado dentro de um enclave protegido por hardware (TEE); a resposta é criptografada antes de sair do enclave. Nenhum dado do prompt pode ser lido pelo provedor do modelo ou pela infraestrutura dele.",
    "lib.modelPrivacy.privateDescription":
      "Modelo privado com retenção zero de dados. Nenhum dado do prompt é armazenado, compartilhado com terceiros ou usado para treinamento.",
    "lib.modelPrivacy.anonymousDescription":
      "O provedor do modelo pode reter prompts, mas eles são anonimizados. Sua identidade é removida antes que qualquer coisa saia do Clovy. Para conteúdo sensível, escolha um modelo privado ou E2EE.",

    "lib.modelPricing.inOut": "{input} entrada / {output} saída",
    "lib.modelPricing.perSecondAudio": "{price} por segundo de áudio",
    "lib.modelPricing.perMillionTokens": "{input} entrada / {output} saída por 1M de tokens",

    "lib.localGeneration.unavailableDescription": "Este modelo local não está mais configurado.",

    "lib.thinking.low.label": "Baixo",
    "lib.thinking.low.blurb": "Respostas mais rápidas com menor uso.",
    "lib.thinking.medium.label": "Médio",
    "lib.thinking.medium.blurb": "Equilibra velocidade e profundidade para a maioria das tarefas.",
    "lib.thinking.high.label": "Alto",
    "lib.thinking.high.blurb": "Raciocínio mais profundo com maior uso.",

    "lib.suggested.autoEconomy": "Economia",
    "lib.suggested.autoQuality": "Qualidade",
    "lib.suggested.autoBalanced": "Equilibrado",
    "lib.suggested.glm52":
      "Escolha padrão: o carro-chefe mais recente do GLM, com raciocínio forte, uso de ferramentas, saída estruturada e retenção zero de dados.",
    "lib.suggested.kimiK3":
      "O carro-chefe mais novo do Kimi: raciocínio multimodal, uso de ferramentas e janela de contexto de 1M de tokens no modo anônimo.",
    "lib.suggested.kimiK26":
      "Opção privada do Kimi: raciocínio multimodal, uso de ferramentas e janela de contexto de 256K tokens com retenção zero de dados.",
    "lib.suggested.glm51":
      "Alternativa estável do GLM: o carro-chefe anterior do GLM, com programação agêntica de ponta, uso de ferramentas e retenção zero de dados.",
    "lib.suggested.parakeet":
      "Rápido e preciso para ditados e reuniões do dia a dia, retenção zero de dados, faixa de preço mais baixa.",
    "lib.suggested.whisper":
      "A melhor precisão multilíngue pelo mesmo preço baixo, com retenção zero de dados.",
    "lib.suggested.veniceSd35":
      "Escolha padrão: o modelo Stable Diffusion 3.5 da Venice, um modelo privado versátil para imagens do dia a dia.",
    "lib.suggested.zImageTurbo":
      "Escolha mais rápida: gerações rápidas e de baixo custo com retenção zero de dados.",
    "lib.suggested.qwenImage":
      "Escolha de qualidade: ótima renderização de texto e fidelidade ao prompt para imagens detalhadas.",
    "lib.suggested.lustifyV8":
      "Escolha sem censura: o modelo de imagem menos restrito, com retenção zero de dados.",
    "lib.suggested.wan22":
      "Escolha padrão: clipes rápidos de 5 segundos em 720p, a opção de menor custo.",
    "lib.suggested.grokImagineVideo":
      "Escolha fotorrealista: clipes realistas com áudio, retenção zero de dados.",
    "lib.suggested.ltx2": "Escolha de qualidade: modelo open source mais detalhado, com áudio.",

    "lib.imageModels.veniceSd35": "O modelo de imagem Stable Diffusion 3.5 padrão da Venice.",
    "lib.imageModels.flux2Pro": "Modelo FLUX de alto detalhe para resultados fotorrealistas.",
    "lib.imageModels.qwenImage": "Ótima renderização de texto e fidelidade ao prompt.",
    "lib.imageModels.chroma": "Modelo de imagem versátil para uso geral.",
    "lib.videoModels.wan22":
      "Modelo padrão de texto para vídeo para clipes rápidos de 5 segundos em 720p.",
    "lib.videoModels.grokImagine": "Clipes fotorrealistas com áudio.",
    "lib.videoModels.ltx2": "Modelo open source mais detalhado, com áudio.",

    "lib.home.message": "Mensagem do Início",
    "lib.home.todayAt": "Hoje às {time}",
    "lib.home.yesterdayAt": "Ontem às {time}",
    "lib.home.dayAt": "{day} às {time}",
    "lib.home.greetingWithName": "{salutation}, {name}",
    "lib.home.goodMorning": "Bom dia",
    "lib.home.goodAfternoon": "Boa tarde",
    "lib.home.goodEvening": "Boa noite",
    "lib.home.pickUpToday": "O que vamos retomar hoje?",
    "lib.home.helpToday": "Com o que você quer ajuda hoje?",
    "lib.home.pickUpAfternoon": "O que vamos retomar hoje à tarde?",
    "lib.home.helpAfternoon": "Com o que você quer ajuda hoje à tarde?",
    "lib.home.pickUpEvening": "O que vamos retomar hoje à noite?",
    "lib.home.helpEvening": "Com o que você quer ajuda hoje à noite?",
    "lib.home.nudge.planMyDay": "Planejar meu dia",
    "lib.home.nudge.thinkThroughDecision": "Pensar em uma decisão",
    "lib.home.nudge.getSomethingDone": "Me ajude a concluir algo",
    "lib.home.nudge.planRestOfDay": "Planejar o resto do meu dia",
    "lib.home.nudge.workThroughBlocker": "Resolver um bloqueio",
    "lib.home.nudge.prioritize": "Me ajude a priorizar",
    "lib.home.nudge.reviewMyDay": "Revisar meu dia",
    "lib.home.nudge.planTomorrow": "Planejar amanhã",
    "lib.home.checkIn": "{salutation}. {question}",

    "lib.notePdf.defaultTitle": "Notas da reunião",
  },
});
