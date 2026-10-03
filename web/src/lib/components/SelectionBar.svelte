<script lang="ts">
	import { mailAction } from '#lib/swipe.ts';
	import FolderPicker from './FolderPicker.svelte';

	let {
		threadIds = [],
		messageIds = [],
		accountIds,
		total,
		archivable = true,
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
		/** Off in the Archive list, where everything is archived already. */
		archivable?: boolean;
		onselectall: () => void;
		onclear: () => void;
		/** Called after an action succeeded, to drop the selection and reload. */
		ondone: () => void;
	} = $props();

	const count = $derived(threadIds.length + messageIds.length);
	const things = $derived(`${count} ${threadIds.length ? 'conversation' : 'mail'}${count === 1 ? '' : 's'}`);

	let busy = $state(false);
	let error = $state('');
	let picker: FolderPicker;

	async function run(action: 'read' | 'unread' | 'archive' | 'trash' | 'move', done: string, extra: Record<string, unknown> = {}) {
		busy = true;
		error = '';
		try {
			await mailAction(action, { threadIds, messageIds }, `${done} ${things}`, extra);
			ondone();
		} catch (e) {
			error = (e as Error).message;
		} finally {
			busy = false;
		}
	}

	async function move() {
		const choice = await picker.choose(accountIds, things);
		if (choice) await run('move', `Moved to ${choice.folder}:`, { account_id: choice.accountId, folder: choice.folder });
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
		{#if archivable}
			<button class="btn small" disabled={busy} onclick={() => run('archive', 'Archived:')}>Archive</button>
		{/if}
		<button class="btn small" disabled={busy} onclick={move}>Move to folder</button>
		<button class="btn small danger" disabled={busy} onclick={() => run('trash', 'Moved to Trash:')}>
			Move to Trash
		</button>
		<button class="btn small quiet" onclick={onclear}>Clear selection</button>
		{#if error}<p class="error" role="alert">{error}</p>{/if}
	</div>
{/if}

<FolderPicker bind:this={picker} />

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
	.bar .error {
		flex-basis: 100%;
		margin: 0.2rem 0.3rem 0;
		color: #ffb3c4;
	}
</style>
