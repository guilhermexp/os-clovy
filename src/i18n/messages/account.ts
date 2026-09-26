import { defineMessages } from "../define";

export default defineMessages({
  en: {
    "account.fallbackName": "Account",
    "account.title": "Account",
    "account.signInNotConfigured": "OpenSoftware sign-in is not configured for this build.",

    // Welcome / sign-in gate
    "account.gate.startFailed": "Clovy could not finish starting",
    "account.gate.tryingAgain": "Trying again...",
    "account.gate.subtitleDictation":
      "Record conversations, turn them into notes, and dictate with your OpenSoftware account.",
    "account.gate.subtitle":
      "Record conversations and turn them into notes with your OpenSoftware account.",
    "account.gate.signInIncomplete": "Sign-in did not complete. Please try again.",
    "account.gate.welcome": "Welcome to Clovy",
    "account.gate.completeInBrowser": "Complete sign-in in browser",
    "account.gate.continue": "Continue with OpenSoftware",
    "account.gate.terms":
      "By continuing, you agree to the <terms>Terms</terms> and <privacy>Privacy Policy</privacy>.",

    // Account settings
    "account.settings.localDescription":
      "Local mode is active. Clovy uses your local Clovy API without OpenSoftware sign-in or billing.",
    "account.settings.description":
      "Sign in with OpenSoftware to use your shared identity and balance across the network.",
    "account.settings.avatarUpdatedDevice": "Avatar updated on this device",
    "account.settings.avatarUpdatedEverywhere": "Avatar updated everywhere",
    "account.settings.avatarPermission":
      "Avatar changed on this device, but it couldn't sync. Sign out and sign in again to update your account permissions.",
    "account.settings.avatarSyncUnavailable":
      "Avatar changed on this device, but syncing isn't available in this OS Accounts environment yet.",
    "account.settings.openingBrowser": "Opening your browser to sign in…",
    "account.settings.signedInAs": "Signed in as {name}.",
    "account.settings.signedOut": "Signed out.",
    "account.settings.localMode": "Local mode",
    "account.settings.checkingSignIn": "Checking sign-in...",
    "account.settings.notSignedIn": "Not signed in",
    "account.settings.localRequests":
      "Requests use your local Clovy API. No OpenSoftware account is used.",
    "account.settings.managedBy": "Your login is managed by OpenSoftware.",
    "account.settings.signOut": "Sign out",
    "account.settings.signIn": "Sign in with OpenSoftware",
    "account.settings.avatar": "Avatar",
    "account.settings.avatarLocal": "A generated pattern saved on this device.",
    "account.settings.avatarLocalOnly": "This pattern is saved only on this device.",
    "account.settings.avatarSynced": "A generated pattern synced with your OpenSoftware account.",
    "account.settings.refresh": "Refresh",
    "account.settings.signedIn": "Signed in",

    // Billing
    "account.billing.title": "Billing",
    "account.billing.description": "Manage usage and subscription details in OpenSoftware.",
    "account.billing.checkoutOpened": "Opened checkout in your browser.",
    "account.billing.portalOpened": "Opened your account portal in the browser.",
    "account.billing.freePlan": "Free plan",
    "account.billing.proPlan": "Pro plan",
    "account.billing.maxPlan": "Max plan",
    "account.billing.noCard": "No credit card required.",
    "account.billing.updateInPortal": "Update billing in your account portal.",
    "account.billing.billingStarts": "Billing starts {date}",
    "account.billing.renews": "Renews {date}",
    "account.billing.freeTrial": "Free trial",
    "account.billing.active": "Active",
    "account.billing.manage": "Manage billing",
    "account.billing.upgradeMax": "Upgrade to Max",
    "account.billing.upgradePro": "Upgrade to Pro",
    "account.billing.beyondPro": "For those who want to go beyond Pro",
    "account.billing.usageRemaining": "Usage remaining",
    "account.billing.refreshUsage": "Refresh usage",

    // Funding notice
    "account.funding.recoveryBody":
      "Your payment needs attention. Update billing to keep using Clovy.",
    "account.funding.recoveryWaiting": "Waiting for your billing update",
    "account.funding.recoveryReopen": "Reopen billing",
    "account.funding.proBody":
      "You have used your Pro credits for this cycle. Max has 5x the monthly usage.",
    "account.funding.topUpBody": "Your credit balance is below zero. Top up to keep using Clovy.",
    "account.funding.topUpCta": "Top up credits",
    "account.funding.topUpWaiting": "Waiting for your top-up",
    "account.funding.topUpReopen": "Reopen account portal",
    "account.funding.starterBody": "Your starter credits are used up. Upgrade to keep using Clovy.",
    "account.funding.starterWaiting": "Waiting for your upgrade",
    "account.funding.starterReopen": "Reopen checkout",
    "account.funding.autoVenice":
      "Auto can route beyond Venice, so it uses Clovy credits. Your Venice API key applies only when you select a Venice model.",
    "account.funding.closedStripe": "I closed the Stripe page",
    "account.funding.checking": "Checking...",
    "account.funding.checkAgain": "Check again",
    "account.funding.selectVenice": "Select a Venice model",
    "account.funding.goMax": "Or go Max",
    "account.funding.paymentAttention": "Payment needs attention",
    "account.funding.outOfCredits": "Out of credits",

    // Referral nudge
    "account.referral.meetingsTitle": "Five meetings, all captured",
    "account.referral.meetingsBody":
      "Know someone who lives in meetings? They get a free month of Clovy, and when they subscribe, so do you.",
    "account.referral.agentTitle": "Give a month, get a month",
    "account.referral.agentBody":
      "Share Clovy with a friend. They get a free month, and when they subscribe, so do you.",
    "account.referral.dictationTitle": "Twenty-five dictations in",
    "account.referral.dictationBody":
      "Know someone who types too much? They get a free month of Clovy, and when they subscribe, so do you.",
    "account.referral.feedbackTitle": "Glad you're enjoying Clovy",
    "account.referral.feedbackBody":
      "Share it with a friend. They get a free month, and when they subscribe, so do you.",
    "account.referral.dismiss": "Dismiss",
    "account.referral.invite": "Invite friends",
  },
  "pt-BR": {
    "account.fallbackName": "Conta",
    "account.title": "Conta",
    "account.signInNotConfigured": "O login com OpenSoftware não está configurado nesta versão.",

    "account.gate.startFailed": "O Clovy não conseguiu terminar de iniciar",
    "account.gate.tryingAgain": "Tentando de novo...",
    "account.gate.subtitleDictation":
      "Grave conversas, transforme-as em notas e dite com sua conta OpenSoftware.",
    "account.gate.subtitle": "Grave conversas e transforme-as em notas com sua conta OpenSoftware.",
    "account.gate.signInIncomplete": "O login não foi concluído. Tente de novo.",
    "account.gate.welcome": "Boas-vindas ao Clovy",
    "account.gate.completeInBrowser": "Conclua o login no navegador",
    "account.gate.continue": "Continuar com OpenSoftware",
    "account.gate.terms":
      "Ao continuar, você concorda com os <terms>Termos</terms> e a <privacy>Política de privacidade</privacy>.",

    "account.settings.localDescription":
      "O modo local está ativo. O Clovy usa sua Clovy API local, sem login nem cobrança da OpenSoftware.",
    "account.settings.description":
      "Entre com OpenSoftware para usar sua identidade e seu saldo compartilhados em toda a rede.",
    "account.settings.avatarUpdatedDevice": "Avatar atualizado neste dispositivo",
    "account.settings.avatarUpdatedEverywhere": "Avatar atualizado em todos os lugares",
    "account.settings.avatarPermission":
      "O avatar mudou neste dispositivo, mas não foi possível sincronizar. Saia e entre de novo para atualizar as permissões da sua conta.",
    "account.settings.avatarSyncUnavailable":
      "O avatar mudou neste dispositivo, mas a sincronização ainda não está disponível neste ambiente do OS Accounts.",
    "account.settings.openingBrowser": "Abrindo o navegador para você entrar…",
    "account.settings.signedInAs": "Conectado como {name}.",
    "account.settings.signedOut": "Você saiu.",
    "account.settings.localMode": "Modo local",
    "account.settings.checkingSignIn": "Verificando login...",
    "account.settings.notSignedIn": "Não conectado",
    "account.settings.localRequests":
      "As solicitações usam sua Clovy API local. Nenhuma conta OpenSoftware é usada.",
    "account.settings.managedBy": "Seu login é gerenciado pela OpenSoftware.",
    "account.settings.signOut": "Sair",
    "account.settings.signIn": "Entrar com OpenSoftware",
    "account.settings.avatar": "Avatar",
    "account.settings.avatarLocal": "Um padrão gerado e salvo neste dispositivo.",
    "account.settings.avatarLocalOnly": "Este padrão está salvo apenas neste dispositivo.",
    "account.settings.avatarSynced": "Um padrão gerado e sincronizado com sua conta OpenSoftware.",
    "account.settings.refresh": "Gerar outro",
    "account.settings.signedIn": "Conectado",

    "account.billing.title": "Cobrança",
    "account.billing.description": "Gerencie o uso e os detalhes da assinatura na OpenSoftware.",
    "account.billing.checkoutOpened": "O checkout foi aberto no seu navegador.",
    "account.billing.portalOpened": "O portal da sua conta foi aberto no navegador.",
    "account.billing.freePlan": "Plano gratuito",
    "account.billing.proPlan": "Plano Pro",
    "account.billing.maxPlan": "Plano Max",
    "account.billing.noCard": "Não é necessário cartão de crédito.",
    "account.billing.updateInPortal": "Atualize a cobrança no portal da sua conta.",
    "account.billing.billingStarts": "A cobrança começa em {date}",
    "account.billing.renews": "Renova em {date}",
    "account.billing.freeTrial": "Teste gratuito",
    "account.billing.active": "Ativo",
    "account.billing.manage": "Gerenciar cobrança",
    "account.billing.upgradeMax": "Fazer upgrade para o Max",
    "account.billing.upgradePro": "Fazer upgrade para o Pro",
    "account.billing.beyondPro": "Para quem quer ir além do Pro",
    "account.billing.usageRemaining": "Uso restante",
    "account.billing.refreshUsage": "Atualizar uso",

    "account.funding.recoveryBody":
      "Seu pagamento precisa de atenção. Atualize a cobrança para continuar usando o Clovy.",
    "account.funding.recoveryWaiting": "Aguardando a atualização da cobrança",
    "account.funding.recoveryReopen": "Reabrir cobrança",
    "account.funding.proBody":
      "Você usou seus créditos Pro deste ciclo. O Max tem 5x o uso mensal.",
    "account.funding.topUpBody":
      "Seu saldo de créditos está abaixo de zero. Recarregue para continuar usando o Clovy.",
    "account.funding.topUpCta": "Recarregar créditos",
    "account.funding.topUpWaiting": "Aguardando sua recarga",
    "account.funding.topUpReopen": "Reabrir portal da conta",
    "account.funding.starterBody":
      "Seus créditos iniciais acabaram. Faça upgrade para continuar usando o Clovy.",
    "account.funding.starterWaiting": "Aguardando seu upgrade",
    "account.funding.starterReopen": "Reabrir checkout",
    "account.funding.autoVenice":
      "O Auto pode usar modelos além da Venice, então consome créditos do Clovy. Sua chave de API da Venice só vale quando você seleciona um modelo Venice.",
    "account.funding.closedStripe": "Fechei a página do Stripe",
    "account.funding.checking": "Verificando...",
    "account.funding.checkAgain": "Verificar de novo",
    "account.funding.selectVenice": "Selecionar um modelo Venice",
    "account.funding.goMax": "Ou vá de Max",
    "account.funding.paymentAttention": "O pagamento precisa de atenção",
    "account.funding.outOfCredits": "Sem créditos",

    "account.referral.meetingsTitle": "Cinco reuniões, todas registradas",
    "account.referral.meetingsBody":
      "Conhece alguém que vive em reuniões? A pessoa ganha um mês grátis do Clovy e, quando assinar, você também ganha.",
    "account.referral.agentTitle": "Dê um mês, ganhe um mês",
    "account.referral.agentBody":
      "Compartilhe o Clovy com um amigo. Ele ganha um mês grátis e, quando assinar, você também ganha.",
    "account.referral.dictationTitle": "Vinte e cinco ditados feitos",
    "account.referral.dictationBody":
      "Conhece alguém que digita demais? A pessoa ganha um mês grátis do Clovy e, quando assinar, você também ganha.",
    "account.referral.feedbackTitle": "Que bom que você está curtindo o Clovy",
    "account.referral.feedbackBody":
      "Compartilhe com um amigo. Ele ganha um mês grátis e, quando assinar, você também ganha.",
    "account.referral.dismiss": "Dispensar",
    "account.referral.invite": "Convidar amigos",
  },
});
