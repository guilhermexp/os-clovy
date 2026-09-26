import { defineMessages } from "../define";

export default defineMessages({
  en: {
    // Tab titles and breadcrumb roots
    "app.nav.home": "Home",
    "app.nav.projects": "Projects",
    "app.nav.sessions": "Sessions",
    "app.nav.allNotes": "All notes",
    "app.nav.routines": "Routines",
    "app.nav.dictation": "Dictation",
    "app.nav.notes": "Notes",
    "app.nav.meetingNotes": "Meeting notes",
    "app.newNote": "New note",
    "app.newSession": "New session",
    "app.breadcrumb.backTo": "Back to {name}",
    "app.breadcrumb.backToRoutines": "Back to routines",
    "app.breadcrumb.backToSessions": "Back to sessions",
    "app.breadcrumb.backToMeetingNotes": "Back to meeting notes",

    // Funding gates
    "app.funding.composer": "Add credits to send messages or generate images and videos.",
    "app.funding.recording":
      "Add credits before starting a recording. You can still browse and edit.",
    "app.funding.noteRetry": "Add credits before retrying note generation.",
    "app.funding.recovery":
      "Add credits before recovering this recording. Your saved audio will stay available.",
    "app.funding.routine": "Add credits before running a routine.",

    // Recording
    "app.recording.startExpired":
      "Recording did not start in time. Open meeting notes and select Record to try again.",
    "app.recording.micNotReady": "Microphone is not ready.",
    "app.recording.didNotStart": "Recording did not start.",
    "app.recording.stopBeforeDeletingNote": "Stop the current recording before deleting a note.",
    "app.recording.stopBeforeDeletingMeetings":
      "Stop the current recording before deleting meetings.",
    "app.inactivity.title": "Still in a meeting?",
    "app.inactivity.description": "Clovy has not heard meeting audio for a while.",
    "app.inactivity.pause": "Pause recording",
    "app.inactivity.keep": "Keep recording",
    "app.inactivity.countdown": {
      one: "Clovy will pause this recording in {count} seconds if you do not answer.",
      other: "Clovy will pause this recording in {count} seconds if you do not answer.",
    },

    // Updates
    "app.update.upToDate": "Clovy is up to date.",
    "app.update.unknownError": "An unknown error occurred.",
    "app.update.checking": "Checking for updates...",
    "app.update.downloading": "Downloading update...",
    "app.update.preparing": "Preparing update...",
    "app.update.failed": "Update failed: {message}",
    "app.update.checkFailed": "Update check failed: {message}",
    "app.update.relaunchFailed": "Relaunch failed: {message}",
    "app.update.relaunchAria": "Relaunch to update to Clovy {version}",
    "app.update.relaunching": "Relaunching...",
    "app.update.relaunch": "Relaunch to update",
    "app.update.hideProgress": "Hide update progress",
    "app.update.dismissStatus": "Dismiss update status",

    // Shell chrome
    "app.sidebar.show": "Show sidebar",
    "app.sidebar.hide": "Hide sidebar",
    "app.sidebar.resize": "Resize sidebar",
    "app.sidebar.toggle": "Toggle sidebar",
    "app.gate.startingAria": "Starting Clovy",
    "app.gate.starting": "Starting Clovy...",
    "app.workspace.loading": "Loading view",
    "app.workspace.loadFailedTitle": "Couldn't open this view",
    "app.workspace.loadFailedBody": "Clovy couldn't load this part of the app. Try again.",
    "app.workspace.openingNote": "Opening note",
    "app.workspace.couldNotOpen": "Could not open {title}.",

    // Notes, sessions, projects
    "app.deleteNote.title": "Delete note?",
    "app.deleteNote.description":
      "This permanently deletes the note and its transcript. This can't be undone.",
    "app.deleteNote.confirm": "Delete note",
    "app.audio.downloaded": "Audio downloaded",
    "app.audio.showFile": "Show file",
    "app.session.renameFailed": "Could not save the session name. It may revert after a restart.",
    "app.session.starting": "Clovy is starting.",
    "app.projects.added": { one: "{count} project added", other: "{count} projects added" },
  },
  "pt-BR": {
    "app.nav.home": "Início",
    "app.nav.projects": "Projetos",
    "app.nav.sessions": "Sessões",
    "app.nav.allNotes": "Todas as notas",
    "app.nav.routines": "Rotinas",
    "app.nav.dictation": "Ditado",
    "app.nav.notes": "Notas",
    "app.nav.meetingNotes": "Notas de reunião",
    "app.newNote": "Nova nota",
    "app.newSession": "Nova sessão",
    "app.breadcrumb.backTo": "Voltar para {name}",
    "app.breadcrumb.backToRoutines": "Voltar para rotinas",
    "app.breadcrumb.backToSessions": "Voltar para sessões",
    "app.breadcrumb.backToMeetingNotes": "Voltar para notas de reunião",

    "app.funding.composer": "Adicione créditos para enviar mensagens ou gerar imagens e vídeos.",
    "app.funding.recording":
      "Adicione créditos antes de iniciar uma gravação. Você ainda pode navegar e editar.",
    "app.funding.noteRetry": "Adicione créditos antes de tentar gerar a nota de novo.",
    "app.funding.recovery":
      "Adicione créditos antes de recuperar esta gravação. O áudio salvo continuará disponível.",
    "app.funding.routine": "Adicione créditos antes de executar uma rotina.",

    "app.recording.startExpired":
      "A gravação não começou a tempo. Abra as notas de reunião e selecione Gravar para tentar de novo.",
    "app.recording.micNotReady": "O microfone não está pronto.",
    "app.recording.didNotStart": "A gravação não começou.",
    "app.recording.stopBeforeDeletingNote": "Pare a gravação atual antes de excluir uma nota.",
    "app.recording.stopBeforeDeletingMeetings": "Pare a gravação atual antes de excluir reuniões.",
    "app.inactivity.title": "Ainda está em uma reunião?",
    "app.inactivity.description": "O Clovy não ouve o áudio da reunião há algum tempo.",
    "app.inactivity.pause": "Pausar gravação",
    "app.inactivity.keep": "Continuar gravando",
    "app.inactivity.countdown": {
      one: "O Clovy vai pausar esta gravação em {count} segundo se você não responder.",
      other: "O Clovy vai pausar esta gravação em {count} segundos se você não responder.",
    },

    "app.update.upToDate": "O Clovy está atualizado.",
    "app.update.unknownError": "Ocorreu um erro desconhecido.",
    "app.update.checking": "Procurando atualizações...",
    "app.update.downloading": "Baixando atualização...",
    "app.update.preparing": "Preparando atualização...",
    "app.update.failed": "Falha na atualização: {message}",
    "app.update.checkFailed": "Falha ao procurar atualizações: {message}",
    "app.update.relaunchFailed": "Falha ao reiniciar: {message}",
    "app.update.relaunchAria": "Reiniciar para atualizar para o Clovy {version}",
    "app.update.relaunching": "Reiniciando...",
    "app.update.relaunch": "Reiniciar para atualizar",
    "app.update.hideProgress": "Ocultar progresso da atualização",
    "app.update.dismissStatus": "Dispensar status da atualização",

    "app.sidebar.show": "Mostrar barra lateral",
    "app.sidebar.hide": "Ocultar barra lateral",
    "app.sidebar.resize": "Redimensionar barra lateral",
    "app.sidebar.toggle": "Alternar barra lateral",
    "app.gate.startingAria": "Iniciando o Clovy",
    "app.gate.starting": "Iniciando o Clovy...",
    "app.workspace.loading": "Carregando visualização",
    "app.workspace.loadFailedTitle": "Não foi possível abrir esta visualização",
    "app.workspace.loadFailedBody":
      "O Clovy não conseguiu carregar esta parte do app. Tente de novo.",
    "app.workspace.openingNote": "Abrindo nota",
    "app.workspace.couldNotOpen": "Não foi possível abrir {title}.",

    "app.deleteNote.title": "Excluir nota?",
    "app.deleteNote.description":
      "Isso exclui permanentemente a nota e a transcrição. Não é possível desfazer.",
    "app.deleteNote.confirm": "Excluir nota",
    "app.audio.downloaded": "Áudio baixado",
    "app.audio.showFile": "Mostrar arquivo",
    "app.session.renameFailed":
      "Não foi possível salvar o nome da sessão. Ele pode voltar ao anterior depois de reiniciar.",
    "app.session.starting": "O Clovy está iniciando.",
    "app.projects.added": {
      one: "{count} projeto adicionado",
      other: "{count} projetos adicionados",
    },
  },
});
