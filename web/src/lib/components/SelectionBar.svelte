<script lang="ts">
	import { mailAction } from '#lib/swipe.ts';
	import DelayMenu from './DelayMenu.svelte';
	import FolderPicker from './FolderPicker.svelte';

	let {
		threadIds = [],
		messageIds = [],
		accountIds,
		archivable = true,
		trashable = true,
		list = 'inbox',
		onclear,
		ondone
	}: {
		/** Selected conversations, or selected single mails: a view uses one of the two. */
		threadIds?: number[];
		messageIds?: number[];
		/** Accounts the selection belongs to; moving to a folder needs exactly one. */
		accountIds: number[];
		/** Unused since the list's own checkbox selects everything; kept so pages need not change. */
		total?: number;
		/** Off in the Archive list, where everything is archived already. */
		archivable?: boolean;
		/** Off in the Trash list. */
		trashable?: boolean;
		/**
		 * Which list the bar serves: 'inbox' offers Important and Delay, 'important' and 'delayed'
		 * offer the way back, 'mixed' (the Inbox page, which also shows Important) offers both
		 * directions, 'other' (Junk, Sent, Archive) offers neither.
		 */
		list?: 'inbox' | 'important' | 'delayed' | 'mixed' | 'other';
		onselectall?: () => void;
		onclear: () => void;
		/** Called after an action succeeded, to drop the selection and reload. */
		ondone: () => void;
	} = $props();

	const count = $derived(threadIds.length + messageIds.length);
	const things = $derived(`${count} ${threadIds.length ? 'conversation' : 'mail'}${count === 1 ? '' : 's'}`);

	let busy = $state(false);
	/** Height of the floating bar; the same room is kept free below the list so nothing hides under it. */
	let barHeight = $state(0);
	let error = $state('');
	let picker: FolderPicker;

	type Action = 'read' | 'unread' | 'important' | 'unimportant' | 'delay' | 'undelay' | 'archive' | 'trash' | 'move';

	async function run(action: Action, done: string, extra: Record<string, unknown> = {}) {
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
	<div class="room" style="height: {barHeight + 24}px"></div>
	<div class="bar" role="toolbar" aria-label="Actions for selected mail" bind:offsetHeight={barHeight}>
		<div class="summary">
			<strong>{things} selected</strong>
			<button class="btn small quiet" onclick={onclear}>Clear selection</button>
		</div>
		<div class="actions">
		<button class="btn small" disabled={busy} onclick={() => run('read', 'Marked as seen:')}>Mark as seen</button>
		<button class="btn small" disabled={busy} onclick={() => run('unread', 'Marked as unseen:')}>Mark as unseen</button>
		{#if list === 'inbox' || list === 'mixed'}
			<button class="btn small" disabled={busy} onclick={() => run('important', 'Moved to Important:')}>
				Move to Important
			</button>
		{/if}
		{#if list === 'important' || list === 'mixed'}
			<button class="btn small" disabled={busy} onclick={() => run('unimportant', 'Moved to Inbox:')}>
				Move to Inbox
			</button>
		{:else if list === 'delayed'}
			<button class="btn small" disabled={busy} onclick={() => run('undelay', 'Back in the Inbox:')}>
				Back to Inbox now
			</button>
		{/if}
		{#if list === 'inbox' || list === 'important' || list === 'mixed'}
			<DelayMenu up disabled={busy} onpick={(days) => run('delay', `Delayed for ${days} ${days === 1 ? 'day' : 'days'}:`, { days })} />
		{/if}
		{#if archivable}
			<button class="btn small" disabled={busy} onclick={() => run('archive', 'Archived:')}>Archive</button>
		{/if}
		<button class="btn small" disabled={busy} onclick={move}>Move to folder</button>
		{#if trashable}
			<button class="btn small danger" disabled={busy} onclick={() => run('trash', 'Moved to Trash:')}>
				Move to Trash
			</button>
		{/if}
		</div>
		{#if error}<p class="error" role="alert">{error}</p>{/if}
	</div>
{/if}

<FolderPicker bind:this={picker} />

<style>
	.room {
		flex: none;
	}
	.bar {
		position: fixed;
		/* Centred with margins, not a transform: a transform would trap the Delay menu inside the bar. */
		left: 0;
		right: 0;
		margin-inline: auto;
		bottom: 1rem;
		z-index: 20;
		width: max-content;
		max-width: calc(100vw - 1.5rem);
		display: grid;
		grid-template-columns: minmax(0, 1fr);
		gap: 0.45rem;
		padding: 0.6rem 0.8rem;
		/* Actions scroll sideways inside the bar; they must not poke out of its rounded corners. */
		overflow: hidden;
		border-radius: var(--radius);
		background: var(--ink);
		color: var(--paper);
		box-shadow: 0 18px 40px -18px color-mix(in srgb, var(--ink) 70%, transparent);
	}
	.summary,
	.actions {
		display: flex;
		align-items: center;
		gap: 0.4rem;
		min-width: 0;
	}
	.summary {
		flex-wrap: wrap;
	}
	/* On a narrow window the actions stay on one line and scroll sideways instead of stacking up. */
	.actions {
		overflow-x: auto;
		scrollbar-width: none;
		margin-inline: -0.8rem;
		padding-inline: 0.8rem;
	}
	.actions::-webkit-scrollbar {
		display: none;
	}
	.summary strong {
		padding: 0 0.3rem;
		margin-right: auto;
		white-space: nowrap;
	}
	/* Buttons of child components (the Delay menu) are styled here too. */
	.bar :global(.btn) {
		color: var(--ink);
		flex: none;
	}
	.bar :global(.btn.quiet) {
		color: color-mix(in srgb, var(--paper) 80%, transparent);
	}
	.bar :global(.btn.danger) {
		color: var(--junk);
	}
	.bar .error {
		margin: 0 0.3rem;
		color: #ffb3c4;
	}
</style>
