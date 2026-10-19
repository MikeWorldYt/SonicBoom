<script setup lang="ts">
import { ref } from 'vue';
import SecretReveal from '../components/ui/SecretReveal.vue';
import { t, formatDate } from '../i18n';
import type { AdminData } from '../types';
defineProps<{ data: AdminData }>();
const expiry = ref('');
const expiryError = ref('');
function validateExpiry(event: SubmitEvent) {
  expiryError.value = '';
  if (expiry.value && (!Number.isFinite(Date.parse(expiry.value + 'Z')) || Date.parse(expiry.value + 'Z') <= Date.now())) {
    event.preventDefault();
    expiryError.value = t('Expiry must be a valid future date in UTC.');
    document.getElementById('token-expiry')?.focus();
  }
}
</script>
<template>
<main id="main-content" tabindex="-1" class="admin-workspace">
  <div v-if="data.page === 'login'" class="box">
    <h1>{{ t("Admin Login") }}</h1><p class="login-intro">{{ t("Sign in to manage API tokens.") }}</p><p v-if="data.error" class="error">{{ t(data.error || '') }}</p>
    <form method="post" action="/admin/login">
      <label for="adminId">{{ t("Username") }}</label><input id="adminId" type="text" name="id" :placeholder="t('ID')" required autocomplete="username">
      <label for="adminPassword">{{ t("Password") }}</label><input id="adminPassword" type="password" name="pw" :placeholder="t('Password')" required autocomplete="current-password">
      <button type="submit">{{ t("Login") }}</button>
    </form>
  </div>
  <div v-else-if="data.page === 'error'" class="box">
    <h1>{{ t("Request Rejected") }}</h1><p class="error">{{ t(data.error || '') }}</p><p><a href="/admin">{{ t("Back to admin panel") }}</a></p>
  </div>
  <template v-else>
    <header class="admin-heading"><h1>{{ t("API access") }}</h1>
      <form class="logout-form" method="post" action="/admin/logout">
        <input type="hidden" name="csrf_token" :value="data.csrf"><button type="submit">{{ t("Logout") }}</button>
      </form>
    </header>
    <div v-if="data.newToken" class="new-token">
      <h2>{{ t("New Token Created") }}</h2><p><strong>{{ t("Copy this token now. It cannot be displayed again.") }}</strong></p><SecretReveal id="admin-new-token" :value="data.newToken"/>
    </div>
    <div class="table-scroll" tabindex="0" role="region" :aria-label="t('API tokens')"><table>
      <thead><tr><th>{{ t("ID") }}</th><th>{{ t("Fingerprint") }}</th><th>{{ t("Created") }}</th><th>{{ t("Expires") }}</th><th>{{ t("Status") }}</th><th>{{ t("Action") }}</th></tr></thead>
      <tbody><tr v-for="token in data.tokens" :key="token.id">
        <td><code>{{ token.id }}</code></td><td><code>{{ token.fingerprint }}</code></td><td>{{ formatDate(token.created) }}</td><td>{{ token.expires === 'Never' ? t('Never') : formatDate(token.expires) }}</td><td>{{ t(token.status) }}</td>
        <td><form v-if="!token.revoked" class="inline-form" method="post" :action="`/admin/tokens/${encodeURIComponent(token.id)}/revoke`">
          <input type="hidden" name="csrf_token" :value="data.csrf"><button type="submit">{{ t("Revoke") }}</button>
        </form></td>
      </tr></tbody>
    </table></div>
    <div class="create-form"><h2>{{ t("Create New Token") }}</h2>
      <form method="post" action="/admin/tokens" @submit="validateExpiry">
        <input type="hidden" name="csrf_token" :value="data.csrf">
        <label for="token-expiry">{{ t('Expires (UTC)') }}</label><input id="token-expiry" v-model="expiry" type="datetime-local" name="expires_at" aria-describedby="expiry-help expiry-error" :aria-invalid="!!expiryError || undefined"><p id="expiry-help" class="help-note">{{ t('Enter UTC time (UTC+00:00). Leave blank for no expiry.') }}</p><p v-if="expiry" class="help-note">{{ expiry.replace('T', ' ') }} UTC+00:00</p><p id="expiry-error" class="field-error" role="alert">{{ expiryError }}</p><br><br>
        <button type="submit">{{ t("Generate Token") }}</button>
      </form>
    </div>
  </template>
</main>
</template>
