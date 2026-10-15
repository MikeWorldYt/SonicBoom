<script setup lang="ts">
import type { AdminData } from '../types';
defineProps<{ data: AdminData }>();
</script>
<template>
  <div v-if="data.page === 'login'" class="box">
    <h1>Admin Login</h1><p v-if="data.error" class="error">{{ data.error }}</p>
    <form method="post" action="/admin/login">
      <input type="text" name="id" placeholder="ID" required autocomplete="username">
      <input type="password" name="pw" placeholder="Password" required autocomplete="current-password">
      <button type="submit">Login</button>
    </form>
  </div>
  <div v-else-if="data.page === 'error'" class="box">
    <h1>Request Rejected</h1><p class="error">{{ data.error }}</p><p><a href="/admin">Back to admin panel</a></p>
  </div>
  <template v-else>
    <h1>Token Management
      <form class="logout-form" method="post" action="/admin/logout">
        <input type="hidden" name="csrf_token" :value="data.csrf"><button type="submit">Logout</button>
      </form>
    </h1>
    <div v-if="data.newToken" class="new-token">
      <h2>New Token Created</h2><p><strong>Copy this token now. It cannot be displayed again.</strong></p><p><code>{{ data.newToken }}</code></p>
    </div>
    <table>
      <thead><tr><th>ID</th><th>Fingerprint</th><th>Created</th><th>Expires</th><th>Status</th><th>Action</th></tr></thead>
      <tbody><tr v-for="token in data.tokens" :key="token.id">
        <td><code>{{ token.id }}</code></td><td><code>{{ token.fingerprint }}</code></td><td>{{ token.created }}</td><td>{{ token.expires }}</td><td>{{ token.status }}</td>
        <td><form v-if="!token.revoked" class="inline-form" method="post" :action="`/admin/tokens/${encodeURIComponent(token.id)}/revoke`">
          <input type="hidden" name="csrf_token" :value="data.csrf"><button type="submit">Revoke</button>
        </form></td>
      </tr></tbody>
    </table>
    <div class="create-form"><h2>Create New Token</h2>
      <form method="post" action="/admin/tokens">
        <input type="hidden" name="csrf_token" :value="data.csrf">
        <label>Expires: <input type="datetime-local" name="expires_at"></label><small>(leave blank for no expiry; interpreted as UTC)</small><br><br>
        <button type="submit">Generate Token</button>
      </form>
    </div>
  </template>
</template>
