<script lang="ts">
	import { api } from '#lib/api.ts';
	import { app, loadSession } from '#lib/app.svelte.ts';

	let mode = $state<'login' | 'register'>(app.setupNeeded ? 'register' : 'login');
	let email = $state('');
	let password = $state('');
	let error = $state('');
	let busy = $state(false);

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		busy = true;
		error = '';
		try {
			await api.post(mode === 'login' ? '/login' : '/register', { email, password });
			await loadSession();
		} catch (e) {
			error = (e as Error).message;
		} finally {
			busy = false;
		}
	}
</script>

<main>
	<div class="door">
		<h1>{app.setupNeeded ? 'Set up Portier' : mode === 'login' ? 'Sign in to Portier' : 'Create your user'}</h1>
		<p class="muted">
			{#if app.setupNeeded}
				Nobody has signed in yet. The user you create now manages this installation.
			{:else}
				Mail only reaches you after you have let its sender in.
			{/if}
		</p>
		<form onsubmit={submit}>
			<label class="field">
				Email
				<input type="email" bind:value={email} required autocomplete="username" />
			</label>
			<label class="field">
				Password
				<input
					type="password"
					bind:value={password}
					required
					minlength={mode === 'register' ? 8 : undefined}
					autocomplete={mode === 'login' ? 'current-password' : 'new-password'}
				/>
			</label>
			{#if error}<p class="error" role="alert">{error}</p>{/if}
			<button class="btn primary" disabled={busy}>
				{mode === 'login' ? 'Sign in' : 'Create user'}
			</button>
		</form>
		{#if app.openRegistration && !app.setupNeeded}
			<button class="btn quiet" onclick={() => (mode = mode === 'login' ? 'register' : 'login')}>
				{mode === 'login' ? 'Create a user instead' : 'Sign in instead'}
			</button>
		{/if}
	</div>
</main>

<style>
	main {
		min-height: 100vh;
		display: grid;
		place-items: center;
		padding: 1.5rem;
	}
	.door {
		width: min(24rem, 100%);
		border-top: 10px solid var(--signal);
		background: var(--surface);
		border-radius: var(--radius);
		padding: 2rem;
		border-inline: 1px solid var(--line);
		border-bottom: 1px solid var(--line);
	}
	h1 {
		font-size: 1.75rem;
	}
	form {
		display: grid;
		gap: 1rem;
		margin: 1.5rem 0 0.75rem;
	}
	form .btn {
		justify-content: center;
		padding-block: 0.7rem;
	}
	.error {
		margin: 0;
	}
</style>
