<script lang="ts">
	import { api, type Account } from '#lib/api.ts';
	import { app } from '#lib/app.svelte.ts';
	import type { FolderChoice } from '#lib/swipe.ts';

	let dialog: HTMLDialogElement;
	let what = $state('');
	let account = $state<Account | undefined>();
	let folders = $state<string[] | null>(null);
	let error = $state('');
	let settle: ((choice: FolderChoice) => void) | null = null;

	/** Opens the picker for mail of the given accounts; resolves with the folder, or null if cancelled. */
	export function choose(accountIds: number[], things: string): Promise<FolderChoice> {
		what = things;
		account = accountIds.length === 1 ? app.accounts.find((a) => a.id === accountIds[0]) : undefined;
		folders = null;
		error = '';
		dialog.showModal();
		if (account) {
			const id = account.id;
			api.get<{ folders: string[] }>(`/accounts/${id}/folders`)
				.then((result) => (folders = result.folders))
				.catch((e) => (error = e.message));
		}
		return new Promise((resolve) => (settle = resolve));
	}

	function finish(choice: FolderChoice) {
		settle?.(choice);
		settle = null;
		if (dialog.open) dialog.close();
	}
</script>

<dialog bind:this={dialog} aria-label="Move to folder" onclose={() => finish(null)}>
	<h2>Move {what} to a folder</h2>
	{#if !account}
		<p>
			The selected mails belong to different mail accounts. Select mails of one account at a time to move them to one
			of its folders.
		</p>
	{:else if error}
		<p class="error" role="alert">The folders of {account.label} cannot be listed: {error}</p>
	{:else if folders === null}
		<p class="muted" aria-busy="true">Loading the folders of {account.label}</p>
	{:else}
		<p class="muted">
			Folders of {account.label}. Mail moved out of Inbox, Sent, Junk, Archive and Trash no longer shows in Email Screen; it stays on
			the mail server.
		</p>
		<ul>
			{#each folders as folder}
				<li><button onclick={() => finish({ accountId: account!.id, folder })}>{folder}</button></li>
			{/each}
		</ul>
	{/if}
	<p class="foot"><button class="btn small" onclick={() => finish(null)}>Cancel</button></p>
</dialog>

<style>
	dialog {
		width: min(28rem, 100vw - 2rem);
		max-height: min(34rem, 100vh - 2rem);
		padding: 1.4rem;
		border: 1px solid var(--line);
		border-radius: var(--radius);
		background: var(--surface);
		color: var(--ink);
	}
	dialog::backdrop {
		background: color-mix(in srgb, var(--ink) 45%, transparent);
	}
	ul {
		list-style: none;
		margin: 0.75rem 0;
		padding: 0;
		display: grid;
		gap: 0.15rem;
	}
	li button {
		width: 100%;
		padding: 0.5rem 0.75rem;
		border: 0;
		border-radius: 9px;
		background: none;
		text-align: left;
		font-weight: 600;
		cursor: pointer;
	}
	li button:hover,
	li button:focus-visible {
		background: var(--paper);
	}
	.foot {
		margin: 1rem 0 0;
		text-align: right;
	}
</style>
