<script lang="ts">
	import { api } from '#lib/api.ts';
	import { app, refreshCounts } from '#lib/app.svelte.ts';

	let {
		threadIds = [],
		messageIds = [],
		accountIds,
		total,
		onselectall,
		onclear,
		ondone
	}: {
		/** Selected conversations, or selected single mails: a view uses one of the two. */
		threadIds?: number[];
		messageIds?: number[];
		/** Accounts the selection belongs to; moving to a folder needs exactly one. */
		accountIds: number[];
		/** How many items the view offers, for "Select all". */
		total: number;
		onselectall: () => void;
		onclear: () => void;
		/** Called after an action succeeded, to drop the selection and reload. */
		ondone: () => void;
	} = $props();

	const count = $derived(threadIds.length + messageIds.length);
	const things = $derived(
		`${count} ${threadIds.length ? 'conversation' : 'mail'}${count === 1 ? '' : 's'}`
	);

	let busy = $state(false);
	let error = $state('');
	let notice = $state('');
	let noticeTimer: ReturnType<typeof setTimeout> | undefined;

	let dialog: HTMLDialogElement | undefined = $state();
	let folders = $state<string[] | null>(null);
	let folderError = $state('');
	const account = $derived(accountIds.length === 1 ? app.accounts.find((a) => a.id === accountIds[0]) : undefined);

	async function run(action: string, done: string, extra: Record<string, unknown> = {}) {
		busy = true;
		error = '';
		const label = things;
		try {
			await api.post('/mail/actions', { action, thread_ids: threadIds, message_ids: messageIds, ...extra });
			notice = `${done} ${label}`;
			clearTimeout(noticeTimer);
			noticeTimer = setTimeout(() => (notice = ''), 5000);
			ondone();
			app.tick += 1;
			refreshCounts().catch(() => {});
		} catch (e) {
			error = (e as Error).message;
		} finally {
			busy = false;
		}
	}

	async function chooseFolder() {
		folders = null;
		folderError = '';
		dialog?.showModal();
		if (!account) return;
		try {
			folders = (await api.get<{ folders: string[] }>(`/accounts/${account.id}/folders`)).folders;
		} catch (e) {
			folderError = (e as Error).message;
		}
	}

	async function moveTo(folder: string) {
		dialog?.close();
		await run('move', `Moved to ${folder}:`, { account_id: account!.id, folder });
	}
</script>

{#if count > 0}
	<div class="bar" role="toolbar" aria-label="Actions for selected mail">
		<strong>{things} selected</strong>
		{#if count < total}
			<button class="btn small quiet" onclick={onselectall}>Select all {total}</button>
		{/if}
		<span class="spacer"></span>
		<button class="btn small" disabled={busy} onclick={() => run('read', 'Marked as read:')}>Mark as read</button>
		<button class="btn small" disabled={busy} onclick={() => run('unread', 'Marked as unread:')}>Mark as unread</button>
		<button class="btn small" disabled={busy} onclick={chooseFolder}>Move to folder</button>
		<button class="btn small danger" disabled={busy} onclick={() => run('trash', 'Moved to Trash:')}>
			Move to Trash
		</button>
		<button class="btn small quiet" onclick={onclear}>Clear selection</button>
		{#if error}<p class="error" role="alert">{error}</p>{/if}
	</div>
{:else if notice}
	<div class="bar done" role="status">{notice}</div>
{/if}

<dialog bind:this={dialog} aria-label="Move to folder">
	<h2>Move {things} to a folder</h2>
	{#if !account}
		<p>
			The selected mails belong to different mail accounts. Select mails of one account at a time to move them to one
			of its folders.
		</p>
	{:else if folderError}
		<p class="error" role="alert">The folders of {account.label} cannot be listed: {folderError}</p>
	{:else if folders === null}
		<p class="muted" aria-busy="true">Loading the folders of {account.label}</p>
	{:else}
		<p class="muted">
			Folders of {account.label}. Mail moved out of Inbox, Sent and Junk no longer shows in Email Screen; it stays on
			the mail server.
		</p>
		<ul>
			{#each folders as folder}
				<li><button onclick={() => moveTo(folder)}>{folder}</button></li>
			{/each}
		</ul>
	{/if}
	<p class="foot"><button class="btn small" onclick={() => dialog?.close()}>Cancel</button></p>
</dialog>

<style>
	.bar {
		position: fixed;
		left: 50%;
		bottom: 1rem;
		transform: translateX(-50%);
		z-index: 20;
		width: min(var(--column), 100vw - 1.5rem);
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 0.4rem;
		padding: 0.6rem 0.8rem;
		border-radius: var(--radius);
		background: var(--ink);
		color: var(--paper);
		box-shadow: 0 18px 40px -18px color-mix(in srgb, var(--ink) 70%, transparent);
	}
	.bar strong {
		padding: 0 0.3rem;
	}
	.bar .btn {
		color: var(--ink);
	}
	.bar .btn.quiet {
		color: color-mix(in srgb, var(--paper) 80%, transparent);
	}
	.bar .btn.danger {
		color: var(--junk);
	}
	.spacer {
		flex: 1;
	}
	.bar.done {
		justify-content: center;
		width: auto;
		padding-inline: 1.2rem;
		font-weight: 600;
	}
	.bar .error {
		flex-basis: 100%;
		margin: 0.2rem 0.3rem 0;
		color: #ffb3c4;
	}
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
	dialog ul {
		list-style: none;
		margin: 0.75rem 0;
		padding: 0;
		display: grid;
		gap: 0.15rem;
	}
	dialog li button {
		width: 100%;
		padding: 0.5rem 0.75rem;
		border: 0;
		border-radius: 9px;
		background: none;
		text-align: left;
		font-weight: 600;
		cursor: pointer;
	}
	dialog li button:hover,
	dialog li button:focus-visible {
		background: var(--paper);
	}
	.foot {
		margin: 1rem 0 0;
		text-align: right;
	}
</style>
