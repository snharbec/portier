<script lang="ts">
	import type { ThreadSummary } from '#lib/api.ts';
	import { app } from '#lib/app.svelte.ts';
	import { displayName, hue, shortDate } from '#lib/format.ts';
	import type { Selection } from '#lib/selection.svelte.ts';
	import { swipeLabel, swipeMail, type SwipeAction } from '#lib/swipe.ts';
	import Avatar from './Avatar.svelte';
	import FolderPicker from './FolderPicker.svelte';
	import Swipeable from './Swipeable.svelte';

	let { threads, selection }: { threads: ThreadSummary[]; selection?: Selection } = $props();

	const accountLabel = (id: number) => app.accounts.find((a) => a.id === id)?.label ?? '';

	let picker: FolderPicker;

	function slide(action: SwipeAction, thread: ThreadSummary) {
		swipeMail(
			action,
			{ threadIds: [thread.id], accountId: thread.account_id, unread: thread.unread > 0, what: '1 conversation' },
			picker.choose
		);
	}
</script>

<ul class="sheet">
	{#each threads as thread (thread.id)}
		{@const who = thread.is_outgoing && thread.sender_address
			? displayName(thread.sender_name, thread.sender_address)
			: displayName(thread.from_name, thread.from_addr)}
		<li>
			<Swipeable
				left={app.swipe.left}
				right={app.swipe.right}
				label={(action) => swipeLabel(action, thread.unread > 0)}
				onaction={(action) => slide(action, thread)}
				tinted={selection?.has(thread.id)}
			>
			{#if selection}
				<label class="pick">
					<input
						type="checkbox"
						checked={selection.has(thread.id)}
						onchange={() => selection.toggle(thread.id)}
						aria-label="Select the conversation {thread.subject || '(no subject)'} with {who}"
					/>
				</label>
			{/if}
			<a href="/thread/{thread.id}" class:unread={thread.unread > 0}>
				<Avatar name={who} seed={thread.sender_address ?? thread.from_addr} />
				<span class="main">
					<span class="top">
						<span class="who">{who}</span>
						{#if thread.count > 1}<span class="count">{thread.count}</span>{/if}
						<time>{shortDate(thread.date)}</time>
					</span>
					<span class="subject">{thread.subject || '(no subject)'}</span>
					<span class="snippet">
						{#if thread.is_outgoing}<span class="you">You:</span>{/if}
						{thread.snippet}
					</span>
				</span>
				{#if app.accounts.length > 1}
					<span class="account" style="--h: {hue(String(thread.account_id * 97))}" title={accountLabel(thread.account_id)}
					></span>
				{/if}
			</a>
			</Swipeable>
		</li>
	{/each}
</ul>

<FolderPicker bind:this={picker} />

<style>
	ul {
		list-style: none;
		margin: 0;
		padding: 0;
	}
	li {
		display: flex;
		position: relative;
	}
	li + li {
		border-top: 1px solid var(--line);
	}
	a {
		display: flex;
		gap: 0.9rem;
		align-items: flex-start;
		flex: 1;
		min-width: 0;
		padding: 0.85rem 1rem;
		text-decoration: none;
	}
	a:hover {
		background: color-mix(in srgb, var(--important) 6%, transparent);
	}
	.main {
		display: grid;
		min-width: 0;
		flex: 1;
	}
	.top {
		display: flex;
		align-items: baseline;
		gap: 0.5rem;
	}
	.who {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.count {
		font-size: 0.8rem;
		color: var(--ink-soft);
		border: 1px solid var(--line);
		border-radius: 999px;
		padding: 0 0.4rem;
	}
	time {
		margin-left: auto;
		flex: none;
		font-size: 0.85rem;
		color: var(--ink-soft);
	}
	.subject,
	.snippet {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.snippet {
		color: var(--ink-soft);
		font-size: 0.925rem;
	}
	.you {
		font-weight: 600;
	}
	.unread .who,
	.unread .subject {
		font-weight: 700;
	}
	a.unread::before {
		content: '';
		position: absolute;
		left: 0;
		top: 0.6rem;
		bottom: 0.6rem;
		width: 4px;
		border-radius: 0 4px 4px 0;
		background: var(--important);
	}
	.account {
		flex: none;
		align-self: center;
		width: 0.6rem;
		height: 0.6rem;
		border-radius: 2px;
		background: hsl(var(--h) 65% 55%);
	}
</style>
