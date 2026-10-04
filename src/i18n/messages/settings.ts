import { defineMessages } from "../define";

export default defineMessages({
  en: {
    "settings.appearance.title": "Appearance",
    "settings.appearance.blurb":
      "Choose the language, theme, accent color, text size, and date format Clovy uses.",
    "settings.interfaceLanguage.title": "Interface language",
    "settings.interfaceLanguage.description":
      "The language of Clovy's menus, buttons, and messages. Dictation and note transcription keep their own language setting.",
    "settings.interfaceLanguage.aria": "Interface language: {language}",
    "settings.theme.title": "Theme",
    "settings.theme.description": "Match the system or force light or dark mode.",
    "settings.theme.aria": "App theme",
    "settings.theme.system": "System",
    "settings.theme.light": "Light",
    "settings.theme.dark": "Dark",
    "settings.theme.systemAria": "Match system theme",
    "settings.theme.lightAria": "Use light theme",
    "settings.theme.darkAria": "Use dark theme",
    "settings.textSize.title": "Text size",
    "settings.textSize.description":
      "Make text across the app larger. Affects every label, note, and conversation.",
    "settings.textSize.default": "Default",
    "settings.textSize.large": "Large",
    "settings.textSize.larger": "Larger",
    "settings.textSize.optionAria": "{size} text size",
    "settings.accent.title": "Accent",
    "settings.accent.description":
      "The brand color used across buttons, highlights, and the recorder.",
    "settings.accent.aria": "Accent color: {color}",
    "settings.dateFormat.title": "Date format",
    "settings.dateFormat.description": "Choose how older session dates appear in the sidebar.",
    "settings.dateFormat.aria": "Date format: {format}",
    "settings.dateFormat.system": "System",
    "settings.dateFormat.monthFirst": "Jul 9",
    "settings.dateFormat.dayFirst": "9 Jul",

    "settings.tabs.general": "General",
    "settings.tabs.billing": "Billing",
    "settings.tabs.shortcuts": "Shortcuts",
    "settings.tabs.dictation": "Dictation",
    "settings.tabs.audio": "Audio",
    "settings.tabs.models": "Models",
    "settings.tabs.agent": "Agent",
    "settings.tabs.memory": "Memory",
    "settings.tabs.connectors": "Plugins",
    "settings.tabs.linkedDevices": "Linked devices",
    "settings.tabs.about": "About",
    "settings.page.description": "Manage audio, dictation, AI models, and agent capabilities.",
    "settings.page.navAria": "Settings sections",
    "settings.general.blurb": "Your account and everyday Clovy preferences.",

    "settings.releaseChannel.stable": "Stable",
    "settings.releaseChannel.rc": "Release candidate",
    "settings.releaseChannel.title": "Release channel",
    "settings.releaseChannel.description":
      "Stable is recommended. Release candidate gets early builds for testing.",
    "settings.releaseChannel.switchAria": "Switch to stable now",
    "settings.releaseChannel.switchEyebrow": "Switch to stable now?",
    "settings.releaseChannel.switchBody":
      "Installs {version}, replacing your release candidate build. You'll get {base} when it reaches stable.",
    "settings.releaseChannel.notNow": "Not now",
    "settings.releaseChannel.switch": "Switch to stable",

    "settings.autoPreference.cost": "Economy",
    "settings.autoPreference.balanced": "Balanced",
    "settings.autoPreference.quality": "Quality",
    "settings.autoPreference.title": "Auto preference",
    "settings.autoPreference.description":
      "Choose how Clovy balances model quality and usage cost.",
    "settings.autoPreference.veniceNote":
      "Auto does not use your Venice API key for notes or chat. Choose a Venice model above to use your key for notes and new chats.",
    "settings.autoPreference.updated": "Automatic model preference updated.",

    "settings.shortcuts.blurb":
      "Set the keyboard shortcuts that start dictation and control Clovy.",
    "settings.shortcuts.modifierRequiredWindows": "Shortcut must include Ctrl, Alt, Shift, or Win.",
    "settings.shortcuts.modifierRequiredMac": "Shortcut must include Cmd, Ctrl, Opt, Shift, or Fn.",
    "settings.shortcuts.monitorUnavailable": "Global shortcut monitoring is unavailable.",
    "settings.shortcuts.unavailable": "Dictation shortcut is unavailable.",
    "settings.shortcuts.pressToRecord": "Press the shortcut to record it.",
    "settings.shortcuts.captureFailed": "Shortcut could not be captured.",
    "settings.shortcuts.captureEnded": "Shortcut capture ended.",
    "settings.shortcuts.noTarget": "Shortcut capture returned without an active target.",
    "settings.shortcuts.invalidData": "Shortcut capture returned invalid data.",
    "settings.shortcuts.set": "{shortcut} set to {label}.",
    "settings.shortcuts.pushToTalk": "Push to talk",
    "settings.shortcuts.pushToTalkDescription":
      "Hold this shortcut to dictate, then release to paste.",
    "settings.shortcuts.toggle": "Toggle dictation",
    "settings.shortcuts.toggleDescription": "Press this shortcut to start or stop dictation.",
    "settings.shortcuts.unavailableTitle": "Dictation shortcuts unavailable",
    "settings.shortcuts.unavailableDescription":
      "Global dictation shortcuts are not available on this device.",
    "settings.shortcuts.change": "Change",
    "settings.shortcuts.reset": "Reset",
    "settings.shortcuts.resetAria": "Reset {name} shortcut to default",

    "settings.helper.restarting": "Dictation stopped and is restarting.",
    "settings.helper.failed": "Settings helper failed.",
    "settings.helper.unavailableAria": "Dictation unavailable",
    "settings.helper.relaunchEyebrow": "Relaunch to finish updating",
    "settings.helper.pausedEyebrow": "Dictation paused",
    "settings.helper.pausedUntilRelaunch":
      "Dictation is paused until you relaunch to finish updating.",
    "settings.helper.relaunch": "Relaunch Clovy",

    "settings.dictation.blurb": "Choose the language, microphone, and behavior for dictation.",
    "settings.dictation.languageDescription":
      "Default language hint for note transcription and dictation.",
    "settings.dictation.languageAria": "Default transcription language",
    "settings.dictation.languageSet": "Default transcription language set to {language}.",
    "settings.dictation.languageAuto": "Default transcription language set to auto-detect.",

    "settings.audio.blurbWindows": "Control how Clovy captures microphone audio on this device.",
    "settings.audio.blurb": "Control how Clovy captures meeting and system audio.",
    "settings.audio.microphone": "Microphone",
    "settings.audio.autoDetect": "Auto-detect",
    "settings.audio.microphoneDescription": "Input device used for dictation.",
    "settings.audio.autoDetectUses": "Auto-detect uses {name}.",
    "settings.audio.autoDetectSystem": "Auto-detect uses the current system input.",
    "settings.audio.microphoneSet": "Microphone set to {name}.",
    "settings.audio.microphoneAuto": "Microphone set to auto-detect.",
    "settings.audio.systemAudio": "System audio",
    "settings.audio.systemAudioDescription":
      "Capture audio from other apps along with your microphone.",
    "settings.audio.systemAudioAria": "Capture system audio for notes",

    "settings.micTest.title": "Mic test",
    "settings.micTest.levelAria": "Microphone test level",
    "settings.micTest.startOver": "Start over",
    "settings.micTest.start": "Start test",
    "settings.micTest.recording": "Recording 5-second sample.",
    "settings.micTest.ready": "Sample ready. Check volume.",
    "settings.micTest.idle": "Check your microphone.",
    "settings.micTest.playbackAria": "Microphone test playback progress",
    "settings.micTest.playAria": "Play microphone test sample",
    "settings.micTest.play": "Play sample",
    "settings.micTest.noSample": "Microphone test did not return a playable sample.",
    "settings.micTest.couldNotRecord": "Microphone test could not record.",
    "settings.micTest.playbackUnavailable":
      "Microphone test recorded, but playback is unavailable.",

    "settings.models.blurb": "Choose the models Clovy uses for voice, text, image, and video.",
    "settings.models.voice": "Voice",
    "settings.models.voiceDescription":
      "Choose the model Clovy uses for note transcription and dictation.",
    "settings.models.partitionNote":
      "Showing models for the current data set: {name}. Switch to the default data set to edit global models.",
    "settings.models.transcription": "Transcription",
    "settings.models.transcriptionDescription": "Speech-to-text for note recordings and dictation.",
    "settings.models.moreVoiceAria": "More options for voice",
    "settings.models.moreVoiceDescription": "Advanced voice settings",
    "settings.models.text": "Text",
    "settings.models.textGroupDescription":
      "Choose the model Clovy uses for generated notes and agent responses.",
    "settings.models.textDescription": "Used for generated notes and agent responses.",
    "settings.models.moreTextAria": "More options for text",
    "settings.models.moreTextDescription": "Advanced text settings",
    "settings.models.imageAndVideo": "Image and video",
    "settings.models.mediaDescription":
      "Choose the models Clovy uses when you ask it to generate an image or video.",
    "settings.models.image": "Image",
    "settings.models.imageDescription": "Used when you generate an image from chat.",
    "settings.models.video": "Video",
    "settings.models.videoDescription": "Used when you generate a video from chat.",
    "settings.models.moreMediaAria": "More options for image and video",
    "settings.models.moreMediaDescription": "Advanced image and video settings",
    "settings.models.updated.transcription": "Transcription model updated.",
    "settings.models.updated.image": "Image model updated.",
    "settings.models.updated.video": "Video model updated.",
    "settings.models.updated.generation": "Text model updated.",
    "settings.models.modelLabel.transcription": "transcription model",
    "settings.models.modelLabel.generation": "text model",
    "settings.models.modelLabel.image": "image model",
    "settings.models.modelLabel.video": "video model",
    "settings.models.popoverTitle.transcription": "Transcription model",
    "settings.models.popoverTitle.image": "Image model",
    "settings.models.popoverTitle.video": "Video model",
    "settings.models.changeAria": "Change {model}",
    "settings.models.chooseAria": "Choose {model}",

    "settings.liveTranscription.title": "Live transcription",
    "settings.liveTranscription.description":
      "Show a live transcript while you record. This transcribes audio twice, so it may use extra credits; turning it off shows the transcript only after the recording ends.",
    "settings.liveTranscription.aria": "Show a live transcript while recording",
    "settings.liveTranscription.on":
      "Live transcription on: the transcript streams while you record.",
    "settings.liveTranscription.off":
      "Live transcription off: the transcript appears after the recording ends.",

    "settings.safeMode.title": "Safe mode",
    "settings.safeMode.descriptionWithVideo":
      "Blur adult content in generated and edited images, and hold back video prompts that request it (videos cannot be blurred). On by default; your image and video work stays private either way.",
    "settings.safeMode.description":
      "Blur adult content in generated and edited images. On by default; your image work stays private either way.",
    "settings.safeMode.aria": "Blur adult content in images",
    "settings.safeMode.on": "Safe mode on: adult content is blurred.",
    "settings.safeMode.off": "Safe mode off: images are not filtered.",

    "settings.venice.title": "Venice API key",
    "settings.venice.description":
      "Use your own key for Venice models so Clovy credits are not used. Stored locally and sent only for Venice requests. For least privilege, use an inference-only key.",
    "settings.venice.keySaved": "Key saved.",
    "settings.venice.apiKey": "API key",
    "settings.venice.savedHidden": "Saved key hidden",
    "settings.venice.enterKey": "Enter a Venice API key before saving.",
    "settings.venice.saved": "Venice API key saved.",
    "settings.venice.removed": "Venice API key removed.",
    "settings.venice.autoDialogTitle": "Auto does not use your Venice API key",
    "settings.venice.autoDialogDescription":
      "Notes and chat are billed to Clovy credits while Auto is selected. Switch to {model} to use your key for notes and new chats.",
    "settings.venice.aVeniceModel": "a Venice model",
    "settings.venice.useModel": "Use {model}",
    "settings.venice.keepAuto": "Keep Auto",

    "settings.about.blurb": "Version, release channel, and other details about this copy of Clovy.",
    "settings.about.releaseVersion": "Release version",
    "settings.about.commit": "Commit",
    "settings.about.updates": "Updates",
    "settings.about.updatesDescription": "Check whether a newer version of Clovy is available.",
    "settings.about.checkForUpdates": "Check for updates",
    "settings.about.community": "Community",
    "settings.about.communityDescription": "Join us in the Clovy community on Telegram at {url}.",
    "settings.about.joinCommunity": "Join community",
    "settings.about.verification": "Server verification",
    "settings.about.verificationDescription":
      "Clovy's server runs in a confidential VM. See exactly what code is running and how to verify it yourself.",
    "settings.about.verify": "Verify server",
    "settings.about.reportIssue": "Report an issue",
    "settings.about.reportIssueDescription":
      "Describe the problem, attach files if you have them, and send the report to the Clovy team.",
    "settings.about.replayOnboarding": "Replay onboarding",
    "settings.about.replayOnboardingDescription":
      "Dev only. Forget that onboarding finished and reload into the first-run wizard.",

    "settings.experiments.unlocked": "Experiments are unlocked",
    "settings.experiments.title": "Experiments",
    "settings.experiments.description":
      "Runtime overrides for features that ship dark. They apply to this install only.",
    "settings.experiments.hide": "Hide again",
    "settings.experiments.browserUse": "Browser use",
    "settings.experiments.browserUseDescription":
      "Enable Browser use on this install while the public feature remains off. Turning it off applies fully after Clovy restarts.",
    "settings.experiments.browserUseAria": "Enable experimental Browser use",
    "settings.experiments.companion": "Companion pairing",
    "settings.experiments.companionDescription":
      "Enable Linked devices and the Clovy Companion runtime on this install. Changes apply after Clovy restarts.",
    "settings.experiments.companionPendingOff":
      "Companion pairing remains available until Clovy restarts. It is saved as off for the next launch.",
    "settings.experiments.companionPendingOn":
      "Companion pairing is saved as on and will become available after Clovy restarts.",
    "settings.experiments.companionAria": "Enable experimental Companion pairing",
    "settings.experiments.agentRuntime": "Agent runtime",
    "settings.experiments.agentRuntimeDescription":
      "Restart the agent to apply the Browser use change.",
    "settings.experiments.restarting": "Restarting...",
    "settings.experiments.restart": "Restart agent",
    "settings.experiments.extension": "Browser extension (unpacked)",
    "settings.experiments.extensionDescription":
      "Open chrome://extensions, turn on Developer mode, choose Load unpacked, and select the revealed folder.",
    "settings.experiments.unpacking": "Unpacking...",
    "settings.experiments.unpack": "Unpack and reveal folder",

    "settings.startup.readError": "Could not read the login item state.",
    "settings.startup.updateError": "Could not update the login item. Try again.",
    "settings.startup.title": "Startup",
    "settings.startup.description":
      "Dictation shortcuts and meeting detection only work while Clovy is running.",
    "settings.startup.openAtLogin": "Open Clovy at login",
    "settings.startup.openAtLoginDescription":
      "Start Clovy automatically when you sign in to your computer.",

    "settings.permissions.systemTitle": "System permissions",
    "settings.permissions.audioAccessTitle": "Audio access",
    "settings.permissions.macDescription":
      "macOS access used for recording audio, pasting dictation, and capturing system sound.",
    "settings.permissions.systemAudioDescription":
      "Audio sources available for recording microphone and app audio.",
    "settings.permissions.micOnlyDescription":
      "Audio sources available for recording microphone audio.",
    "settings.permissions.microphoneDescription": "Record dictation and note audio.",
    "settings.permissions.accessibility": "Accessibility",
    "settings.permissions.accessibilityDescription": "Paste dictated text into the active app.",
    "settings.permissions.systemAudioRowDescription":
      "Record audio from other apps when system audio is enabled.",
    "settings.permissions.manage": "Manage",
    "settings.permissions.manageAria": "Manage {name} permission",
    "settings.permissions.status.allowed": "Allowed",
    "settings.permissions.status.blocked": "Blocked",
    "settings.permissions.status.restricted": "Restricted",
    "settings.permissions.status.needsAccess": "Needs access",
    "settings.permissions.status.notRequested": "Not requested",
    "settings.permissions.status.noMicrophone": "No microphone found",
    "settings.permissions.status.unsupported": "Unsupported",
    "settings.permissions.status.unknown": "Unknown",
    "settings.permissions.status.checking": "Checking",
    "settings.permissions.status.available": "Available",
    "settings.permissions.status.unavailable": "Unavailable",

    "settings.privacy.loadError": "Could not load privacy settings.",
    "settings.privacy.on": "Anonymous usage statistics are on for this device.",
    "settings.privacy.off":
      "Anonymous usage statistics are off. Usage data stored on this device was deleted.",
    "settings.privacy.updateError": "Could not update usage statistics. Try again.",
    "settings.privacy.title": "Privacy",
    "settings.privacy.description":
      "Choose whether Clovy shares anonymous usage statistics with OpenSoftware. Off by default.",
    "settings.privacy.share": "Share anonymous usage statistics",
    "settings.privacy.shareDescription":
      "Anonymous counts of feature usage, like how many dictation sessions happen in a week. Never your recordings, notes, or anything you write.",
    "settings.privacy.learnHow": "Learn how it works",

    "settings.style.title": "Style",
    "settings.style.outputStyle": "Output style",
    "settings.style.aria": "Dictation style",
    "settings.style.standard": "Standard",
    "settings.style.casual": "Casual",
    "settings.style.formal": "Formal",
    "settings.style.standardDescription":
      "Sentence case with light cleanup. Keeps your natural tone.",
    "settings.style.casualDescription": "Lowercase sentences, contractions, minimal cleanup.",
    "settings.style.formalDescription":
      "Full words and conventional capitalization. Keeps your wording.",
    "settings.style.standardSample":
      "Got it. Let me know when you're free to chat about the Q3 plan. Happy to jump on in the morning.",
    "settings.style.casualSample":
      "got it. let me know when you're free to chat about the q3 plan. happy to jump on in the morning.",
    "settings.style.formalSample":
      "Got it. Let me know when you are free to chat about the Q3 plan. Happy to jump on in the morning.",

    "settings.dictionary.title": "Dictionary",
    "settings.dictionary.description":
      "Words or phrases Clovy should preserve during transcription.",
    "settings.dictionary.empty":
      "No entries yet. Add words or phrases for transcription to preserve.",
    "settings.dictionary.noMatch": 'No entries match "{query}".',
    "settings.dictionary.searchAria": "Search dictionary",
    "settings.dictionary.add": "Add entry",
    "settings.dictionary.editAria": "Edit {phrase}",
    "settings.dictionary.deleteAria": "Delete {phrase}",
    "settings.dictionary.editTitle": "Edit dictionary entry",
    "settings.dictionary.addTitle": "Add dictionary entry",
    "settings.dictionary.saveChanges": "Save changes",
    "settings.dictionary.field": "Word or phrase",
    "settings.dictionary.placeholder": "e.g. Anthropic, ARR, Jane Doe",

    "settings.personality.loadError": "Unable to load Clovy's personality.",
    "settings.personality.saveError": "Unable to save Clovy's personality.",
    "settings.personality.saved": "Saved",
    "settings.personality.title": "Personality",
    "settings.personality.description":
      "Choose the voice Clovy uses in Home and new agent sessions.",
    "settings.personality.loading": "Loading Clovy's personality",
    "settings.personality.legend": "Choose Clovy's personality",
    "settings.personality.applies": "Applies to future Home replies and the next agent run.",
  },
  "pt-BR": {
    "settings.appearance.title": "Aparência",
    "settings.appearance.blurb":
      "Escolha o idioma, o tema, a cor de destaque, o tamanho do texto e o formato de data do Clovy.",
    "settings.interfaceLanguage.title": "Idioma da interface",
    "settings.interfaceLanguage.description":
      "O idioma dos menus, botões e mensagens do Clovy. Ditado e transcrição de notas mantêm a própria configuração de idioma.",
    "settings.interfaceLanguage.aria": "Idioma da interface: {language}",
    "settings.theme.title": "Tema",
    "settings.theme.description": "Siga o sistema ou force o modo claro ou escuro.",
    "settings.theme.aria": "Tema do app",
    "settings.theme.system": "Sistema",
    "settings.theme.light": "Claro",
    "settings.theme.dark": "Escuro",
    "settings.theme.systemAria": "Seguir o tema do sistema",
    "settings.theme.lightAria": "Usar tema claro",
    "settings.theme.darkAria": "Usar tema escuro",
    "settings.textSize.title": "Tamanho do texto",
    "settings.textSize.description":
      "Aumente o texto em todo o app. Afeta todos os rótulos, notas e conversas.",
    "settings.textSize.default": "Padrão",
    "settings.textSize.large": "Grande",
    "settings.textSize.larger": "Maior",
    "settings.textSize.optionAria": "Tamanho de texto {size}",
    "settings.accent.title": "Destaque",
    "settings.accent.description": "A cor da marca usada em botões, realces e no gravador.",
    "settings.accent.aria": "Cor de destaque: {color}",
    "settings.dateFormat.title": "Formato de data",
    "settings.dateFormat.description":
      "Escolha como as datas de sessões mais antigas aparecem na barra lateral.",
    "settings.dateFormat.aria": "Formato de data: {format}",
    "settings.dateFormat.system": "Sistema",
    "settings.dateFormat.monthFirst": "jul 9",
    "settings.dateFormat.dayFirst": "9 jul",

    "settings.tabs.general": "Geral",
    "settings.tabs.billing": "Cobrança",
    "settings.tabs.shortcuts": "Atalhos",
    "settings.tabs.dictation": "Ditado",
    "settings.tabs.audio": "Áudio",
    "settings.tabs.models": "Modelos",
    "settings.tabs.agent": "Agente",
    "settings.tabs.memory": "Memória",
    "settings.tabs.connectors": "Plugins",
    "settings.tabs.linkedDevices": "Dispositivos vinculados",
    "settings.tabs.about": "Sobre",
    "settings.page.description": "Gerencie áudio, ditado, modelos de IA e recursos do agente.",
    "settings.page.navAria": "Seções das configurações",
    "settings.general.blurb": "Sua conta e as preferências do dia a dia do Clovy.",

    "settings.releaseChannel.stable": "Estável",
    "settings.releaseChannel.rc": "Versão candidata",
    "settings.releaseChannel.title": "Canal de lançamento",
    "settings.releaseChannel.description":
      "O canal estável é o recomendado. A versão candidata recebe builds antecipadas para testes.",
    "settings.releaseChannel.switchAria": "Mudar para a versão estável agora",
    "settings.releaseChannel.switchEyebrow": "Mudar para a versão estável agora?",
    "settings.releaseChannel.switchBody":
      "Instala a {version}, substituindo sua build da versão candidata. Você vai receber a {base} quando ela chegar ao canal estável.",
    "settings.releaseChannel.notNow": "Agora não",
    "settings.releaseChannel.switch": "Mudar para a estável",

    "settings.autoPreference.cost": "Economia",
    "settings.autoPreference.balanced": "Equilibrado",
    "settings.autoPreference.quality": "Qualidade",
    "settings.autoPreference.title": "Preferência do Auto",
    "settings.autoPreference.description":
      "Escolha como o Clovy equilibra a qualidade do modelo e o custo de uso.",
    "settings.autoPreference.veniceNote":
      "O Auto não usa sua chave de API da Venice para notas ou conversas. Escolha um modelo da Venice acima para usar sua chave em notas e novas conversas.",
    "settings.autoPreference.updated": "Preferência do modelo automático atualizada.",

    "settings.shortcuts.blurb":
      "Defina os atalhos de teclado que iniciam o ditado e controlam o Clovy.",
    "settings.shortcuts.modifierRequiredWindows":
      "O atalho precisa incluir Ctrl, Alt, Shift ou Win.",
    "settings.shortcuts.modifierRequiredMac":
      "O atalho precisa incluir Cmd, Ctrl, Opt, Shift ou Fn.",
    "settings.shortcuts.monitorUnavailable":
      "O monitoramento de atalhos globais não está disponível.",
    "settings.shortcuts.unavailable": "O atalho de ditado não está disponível.",
    "settings.shortcuts.pressToRecord": "Pressione o atalho para registrá-lo.",
    "settings.shortcuts.captureFailed": "Não foi possível capturar o atalho.",
    "settings.shortcuts.captureEnded": "Captura de atalho encerrada.",
    "settings.shortcuts.noTarget": "A captura de atalho retornou sem um destino ativo.",
    "settings.shortcuts.invalidData": "A captura de atalho retornou dados inválidos.",
    "settings.shortcuts.set": "{shortcut} definido como {label}.",
    "settings.shortcuts.pushToTalk": "Pressione para falar",
    "settings.shortcuts.pushToTalkDescription": "Segure este atalho para ditar e solte para colar.",
    "settings.shortcuts.toggle": "Alternar ditado",
    "settings.shortcuts.toggleDescription": "Pressione este atalho para iniciar ou parar o ditado.",
    "settings.shortcuts.unavailableTitle": "Atalhos de ditado indisponíveis",
    "settings.shortcuts.unavailableDescription":
      "Os atalhos globais de ditado não estão disponíveis neste dispositivo.",
    "settings.shortcuts.change": "Alterar",
    "settings.shortcuts.reset": "Redefinir",
    "settings.shortcuts.resetAria": "Redefinir o atalho {name} para o padrão",

    "settings.helper.restarting": "O ditado parou e está reiniciando.",
    "settings.helper.failed": "O auxiliar das configurações falhou.",
    "settings.helper.unavailableAria": "Ditado indisponível",
    "settings.helper.relaunchEyebrow": "Reinicie para concluir a atualização",
    "settings.helper.pausedEyebrow": "Ditado pausado",
    "settings.helper.pausedUntilRelaunch":
      "O ditado fica pausado até você reiniciar o app para concluir a atualização.",
    "settings.helper.relaunch": "Reiniciar o Clovy",

    "settings.dictation.blurb": "Escolha o idioma, o microfone e o comportamento do ditado.",
    "settings.dictation.languageDescription":
      "Idioma padrão sugerido para a transcrição de notas e o ditado.",
    "settings.dictation.languageAria": "Idioma padrão de transcrição",
    "settings.dictation.languageSet": "Idioma padrão de transcrição definido como {language}.",
    "settings.dictation.languageAuto":
      "Idioma padrão de transcrição definido para detecção automática.",

    "settings.audio.blurbWindows":
      "Controle como o Clovy capta o áudio do microfone neste dispositivo.",
    "settings.audio.blurb": "Controle como o Clovy capta o áudio de reuniões e do sistema.",
    "settings.audio.microphone": "Microfone",
    "settings.audio.autoDetect": "Detecção automática",
    "settings.audio.microphoneDescription": "Dispositivo de entrada usado para o ditado.",
    "settings.audio.autoDetectUses": "A detecção automática usa {name}.",
    "settings.audio.autoDetectSystem": "A detecção automática usa a entrada atual do sistema.",
    "settings.audio.microphoneSet": "Microfone definido como {name}.",
    "settings.audio.microphoneAuto": "Microfone definido para detecção automática.",
    "settings.audio.systemAudio": "Áudio do sistema",
    "settings.audio.systemAudioDescription":
      "Capte o áudio de outros apps junto com o seu microfone.",
    "settings.audio.systemAudioAria": "Capturar o áudio do sistema para as notas",

    "settings.micTest.title": "Teste do microfone",
    "settings.micTest.levelAria": "Nível do teste do microfone",
    "settings.micTest.startOver": "Recomeçar",
    "settings.micTest.start": "Iniciar teste",
    "settings.micTest.recording": "Gravando uma amostra de 5 segundos.",
    "settings.micTest.ready": "Amostra pronta. Confira o volume.",
    "settings.micTest.idle": "Teste o seu microfone.",
    "settings.micTest.playbackAria": "Progresso da reprodução do teste do microfone",
    "settings.micTest.playAria": "Reproduzir a amostra do teste do microfone",
    "settings.micTest.play": "Reproduzir amostra",
    "settings.micTest.noSample": "O teste do microfone não retornou uma amostra reproduzível.",
    "settings.micTest.couldNotRecord": "Não foi possível gravar o teste do microfone.",
    "settings.micTest.playbackUnavailable":
      "O teste do microfone foi gravado, mas a reprodução não está disponível.",

    "settings.models.blurb": "Escolha os modelos que o Clovy usa para voz, texto, imagem e vídeo.",
    "settings.models.voice": "Voz",
    "settings.models.voiceDescription":
      "Escolha o modelo que o Clovy usa para a transcrição de notas e o ditado.",
    "settings.models.partitionNote":
      "Mostrando os modelos do conjunto de dados atual: {name}. Mude para o conjunto de dados padrão para editar os modelos globais.",
    "settings.models.transcription": "Transcrição",
    "settings.models.transcriptionDescription":
      "Conversão de fala em texto para gravações de notas e ditado.",
    "settings.models.moreVoiceAria": "Mais opções de voz",
    "settings.models.moreVoiceDescription": "Configurações avançadas de voz",
    "settings.models.text": "Texto",
    "settings.models.textGroupDescription":
      "Escolha o modelo que o Clovy usa para notas geradas e respostas do agente.",
    "settings.models.textDescription": "Usado para notas geradas e respostas do agente.",
    "settings.models.moreTextAria": "Mais opções de texto",
    "settings.models.moreTextDescription": "Configurações avançadas de texto",
    "settings.models.imageAndVideo": "Imagem e vídeo",
    "settings.models.mediaDescription":
      "Escolha os modelos que o Clovy usa quando você pede para gerar uma imagem ou um vídeo.",
    "settings.models.image": "Imagem",
    "settings.models.imageDescription": "Usado quando você gera uma imagem pela conversa.",
    "settings.models.video": "Vídeo",
    "settings.models.videoDescription": "Usado quando você gera um vídeo pela conversa.",
    "settings.models.moreMediaAria": "Mais opções de imagem e vídeo",
    "settings.models.moreMediaDescription": "Configurações avançadas de imagem e vídeo",
    "settings.models.updated.transcription": "Modelo de transcrição atualizado.",
    "settings.models.updated.image": "Modelo de imagem atualizado.",
    "settings.models.updated.video": "Modelo de vídeo atualizado.",
    "settings.models.updated.generation": "Modelo de texto atualizado.",
    "settings.models.modelLabel.transcription": "modelo de transcrição",
    "settings.models.modelLabel.generation": "modelo de texto",
    "settings.models.modelLabel.image": "modelo de imagem",
    "settings.models.modelLabel.video": "modelo de vídeo",
    "settings.models.popoverTitle.transcription": "Modelo de transcrição",
    "settings.models.popoverTitle.image": "Modelo de imagem",
    "settings.models.popoverTitle.video": "Modelo de vídeo",
    "settings.models.changeAria": "Alterar {model}",
    "settings.models.chooseAria": "Escolher {model}",

    "settings.liveTranscription.title": "Transcrição ao vivo",
    "settings.liveTranscription.description":
      "Mostra uma transcrição ao vivo enquanto você grava. Isso transcreve o áudio duas vezes, então pode usar créditos extras; se desativar, a transcrição aparece só depois que a gravação termina.",
    "settings.liveTranscription.aria": "Mostrar uma transcrição ao vivo durante a gravação",
    "settings.liveTranscription.on":
      "Transcrição ao vivo ativada: a transcrição aparece enquanto você grava.",
    "settings.liveTranscription.off":
      "Transcrição ao vivo desativada: a transcrição aparece depois que a gravação termina.",

    "settings.safeMode.title": "Modo seguro",
    "settings.safeMode.descriptionWithVideo":
      "Desfoca conteúdo adulto em imagens geradas e editadas e retém prompts de vídeo que peçam esse conteúdo (vídeos não podem ser desfocados). Ativado por padrão; seu trabalho com imagens e vídeos continua privado de qualquer forma.",
    "settings.safeMode.description":
      "Desfoca conteúdo adulto em imagens geradas e editadas. Ativado por padrão; seu trabalho com imagens continua privado de qualquer forma.",
    "settings.safeMode.aria": "Desfocar conteúdo adulto em imagens",
    "settings.safeMode.on": "Modo seguro ativado: conteúdo adulto fica desfocado.",
    "settings.safeMode.off": "Modo seguro desativado: as imagens não são filtradas.",

    "settings.venice.title": "Chave de API da Venice",
    "settings.venice.description":
      "Use sua própria chave nos modelos da Venice para não gastar créditos do Clovy. Ela fica salva localmente e só é enviada em solicitações à Venice. Para o menor privilégio, use uma chave só de inferência.",
    "settings.venice.keySaved": "Chave salva.",
    "settings.venice.apiKey": "Chave de API",
    "settings.venice.savedHidden": "Chave salva oculta",
    "settings.venice.enterKey": "Informe uma chave de API da Venice antes de salvar.",
    "settings.venice.saved": "Chave de API da Venice salva.",
    "settings.venice.removed": "Chave de API da Venice removida.",
    "settings.venice.autoDialogTitle": "O Auto não usa sua chave de API da Venice",
    "settings.venice.autoDialogDescription":
      "Notas e conversas são cobradas em créditos do Clovy enquanto o Auto estiver selecionado. Mude para {model} para usar sua chave em notas e novas conversas.",
    "settings.venice.aVeniceModel": "um modelo da Venice",
    "settings.venice.useModel": "Usar {model}",
    "settings.venice.keepAuto": "Manter o Auto",

    "settings.about.blurb": "Versão, canal de lançamento e outros detalhes desta cópia do Clovy.",
    "settings.about.releaseVersion": "Versão",
    "settings.about.commit": "Commit",
    "settings.about.updates": "Atualizações",
    "settings.about.updatesDescription":
      "Verifique se há uma versão mais recente do Clovy disponível.",
    "settings.about.checkForUpdates": "Buscar atualizações",
    "settings.about.community": "Comunidade",
    "settings.about.communityDescription": "Participe da comunidade do Clovy no Telegram em {url}.",
    "settings.about.joinCommunity": "Entrar na comunidade",
    "settings.about.verification": "Verificação do servidor",
    "settings.about.verificationDescription":
      "O servidor do Clovy roda em uma VM confidencial. Veja exatamente qual código está rodando e como verificar por conta própria.",
    "settings.about.verify": "Verificar servidor",
    "settings.about.reportIssue": "Relatar um problema",
    "settings.about.reportIssueDescription":
      "Descreva o problema, anexe arquivos se tiver e envie o relato para a equipe do Clovy.",
    "settings.about.replayOnboarding": "Repetir a introdução",
    "settings.about.replayOnboardingDescription":
      "Só em desenvolvimento. Esquece que a introdução foi concluída e recarrega no assistente inicial.",

    "settings.experiments.unlocked": "Experimentos desbloqueados",
    "settings.experiments.title": "Experimentos",
    "settings.experiments.description":
      "Substituições em tempo de execução para recursos lançados ocultos. Valem só para esta instalação.",
    "settings.experiments.hide": "Ocultar de novo",
    "settings.experiments.browserUse": "Uso do navegador",
    "settings.experiments.browserUseDescription":
      "Ativa o uso do navegador nesta instalação enquanto o recurso público continua desativado. Desativar só tem efeito completo depois que o Clovy reiniciar.",
    "settings.experiments.browserUseAria": "Ativar o uso do navegador experimental",
    "settings.experiments.companion": "Pareamento do Companion",
    "settings.experiments.companionDescription":
      "Ativa Dispositivos vinculados e o runtime do Clovy Companion nesta instalação. As mudanças valem depois que o Clovy reiniciar.",
    "settings.experiments.companionPendingOff":
      "O pareamento do Companion continua disponível até o Clovy reiniciar. Ele está salvo como desativado para a próxima abertura.",
    "settings.experiments.companionPendingOn":
      "O pareamento do Companion está salvo como ativado e ficará disponível depois que o Clovy reiniciar.",
    "settings.experiments.companionAria": "Ativar o pareamento do Companion experimental",
    "settings.experiments.agentRuntime": "Runtime do agente",
    "settings.experiments.agentRuntimeDescription":
      "Reinicie o agente para aplicar a mudança no uso do navegador.",
    "settings.experiments.restarting": "Reiniciando...",
    "settings.experiments.restart": "Reiniciar agente",
    "settings.experiments.extension": "Extensão do navegador (descompactada)",
    "settings.experiments.extensionDescription":
      "Abra chrome://extensions, ative o Modo do desenvolvedor, escolha Carregar sem compactação e selecione a pasta exibida.",
    "settings.experiments.unpacking": "Descompactando...",
    "settings.experiments.unpack": "Descompactar e mostrar a pasta",

    "settings.startup.readError": "Não foi possível ler o estado do item de login.",
    "settings.startup.updateError": "Não foi possível atualizar o item de login. Tente de novo.",
    "settings.startup.title": "Inicialização",
    "settings.startup.description":
      "Os atalhos de ditado e a detecção de reuniões só funcionam enquanto o Clovy está aberto.",
    "settings.startup.openAtLogin": "Abrir o Clovy ao iniciar a sessão",
    "settings.startup.openAtLoginDescription":
      "Inicia o Clovy automaticamente quando você entra no computador.",

    "settings.permissions.systemTitle": "Permissões do sistema",
    "settings.permissions.audioAccessTitle": "Acesso ao áudio",
    "settings.permissions.macDescription":
      "Acessos do macOS usados para gravar áudio, colar o ditado e captar o som do sistema.",
    "settings.permissions.systemAudioDescription":
      "Fontes de áudio disponíveis para gravar o microfone e o áudio de apps.",
    "settings.permissions.micOnlyDescription":
      "Fontes de áudio disponíveis para gravar o áudio do microfone.",
    "settings.permissions.microphoneDescription": "Grava o áudio do ditado e das notas.",
    "settings.permissions.accessibility": "Acessibilidade",
    "settings.permissions.accessibilityDescription": "Cola o texto ditado no app ativo.",
    "settings.permissions.systemAudioRowDescription":
      "Grava o áudio de outros apps quando o áudio do sistema está ativado.",
    "settings.permissions.manage": "Gerenciar",
    "settings.permissions.manageAria": "Gerenciar a permissão de {name}",
    "settings.permissions.status.allowed": "Permitido",
    "settings.permissions.status.blocked": "Bloqueado",
    "settings.permissions.status.restricted": "Restrito",
    "settings.permissions.status.needsAccess": "Precisa de acesso",
    "settings.permissions.status.notRequested": "Não solicitado",
    "settings.permissions.status.noMicrophone": "Nenhum microfone encontrado",
    "settings.permissions.status.unsupported": "Sem suporte",
    "settings.permissions.status.unknown": "Desconhecido",
    "settings.permissions.status.checking": "Verificando",
    "settings.permissions.status.available": "Disponível",
    "settings.permissions.status.unavailable": "Indisponível",

    "settings.privacy.loadError": "Não foi possível carregar as configurações de privacidade.",
    "settings.privacy.on": "As estatísticas de uso anônimas estão ativadas neste dispositivo.",
    "settings.privacy.off":
      "As estatísticas de uso anônimas estão desativadas. Os dados de uso salvos neste dispositivo foram excluídos.",
    "settings.privacy.updateError":
      "Não foi possível atualizar as estatísticas de uso. Tente de novo.",
    "settings.privacy.title": "Privacidade",
    "settings.privacy.description":
      "Escolha se o Clovy compartilha estatísticas de uso anônimas com a OpenSoftware. Desativado por padrão.",
    "settings.privacy.share": "Compartilhar estatísticas de uso anônimas",
    "settings.privacy.shareDescription":
      "Contagens anônimas do uso de recursos, como quantas sessões de ditado acontecem por semana. Nunca suas gravações, notas ou qualquer coisa que você escreva.",
    "settings.privacy.learnHow": "Saiba como funciona",

    "settings.style.title": "Estilo",
    "settings.style.outputStyle": "Estilo de saída",
    "settings.style.aria": "Estilo do ditado",
    "settings.style.standard": "Padrão",
    "settings.style.casual": "Casual",
    "settings.style.formal": "Formal",
    "settings.style.standardDescription":
      "Maiúscula no início das frases, com uma limpeza leve. Mantém o seu tom natural.",
    "settings.style.casualDescription": "Frases em minúsculas, contrações e limpeza mínima.",
    "settings.style.formalDescription":
      "Palavras completas e maiúsculas convencionais. Mantém as suas palavras.",
    "settings.style.standardSample":
      "Entendi. Me avisa quando puder conversar sobre o plano do T3. Posso entrar de manhã, sem problema.",
    "settings.style.casualSample":
      "entendi. me avisa quando puder conversar sobre o plano do t3. posso entrar de manhã, sem problema.",
    "settings.style.formalSample":
      "Entendido. Avise-me quando estiver disponível para conversar sobre o plano do T3. Posso participar pela manhã, sem problema.",

    "settings.dictionary.title": "Dicionário",
    "settings.dictionary.description":
      "Palavras ou frases que o Clovy deve preservar na transcrição.",
    "settings.dictionary.empty":
      "Nenhuma entrada ainda. Adicione palavras ou frases para a transcrição preservar.",
    "settings.dictionary.noMatch": 'Nenhuma entrada corresponde a "{query}".',
    "settings.dictionary.searchAria": "Buscar no dicionário",
    "settings.dictionary.add": "Adicionar entrada",
    "settings.dictionary.editAria": "Editar {phrase}",
    "settings.dictionary.deleteAria": "Excluir {phrase}",
    "settings.dictionary.editTitle": "Editar entrada do dicionário",
    "settings.dictionary.addTitle": "Adicionar entrada ao dicionário",
    "settings.dictionary.saveChanges": "Salvar alterações",
    "settings.dictionary.field": "Palavra ou frase",
    "settings.dictionary.placeholder": "ex.: Anthropic, ARR, Maria Silva",

    "settings.personality.loadError": "Não foi possível carregar a personalidade do Clovy.",
    "settings.personality.saveError": "Não foi possível salvar a personalidade do Clovy.",
    "settings.personality.saved": "Salvo",
    "settings.personality.title": "Personalidade",
    "settings.personality.description":
      "Escolha a voz que o Clovy usa no Início e em novas sessões do agente.",
    "settings.personality.loading": "Carregando a personalidade do Clovy",
    "settings.personality.legend": "Escolha a personalidade do Clovy",
    "settings.personality.applies":
      "Vale para as próximas respostas no Início e a próxima execução do agente.",
  },
});
