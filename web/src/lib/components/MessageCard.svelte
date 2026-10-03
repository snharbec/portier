<script lang="ts">
	import { goto } from '$app/navigation';
	import type { FileEntry, Message } from '#lib/api.ts';
	import { app, startDraft } from '#lib/app.svelte.ts';
	import { copyText } from '#lib/clipboard.ts';
	import { displayName, fileSize, fullDate, shortDate } from '#lib/format.ts';
	import { untrack } from 'svelte';
	import Avatar from './Avatar.svelte';
	import FileTile from './FileTile.svelte';
	import FileViewer from './FileViewer.svelte';
	import MessageBody from './MessageBody.svelte';

	let {
		message,
		open = true,
		actions = true,
		showSubject = false
	}: { message: Message; open?: boolean; actions?: boolean; showSubject?: boolean } = $props();

	// `open` is only the starting state; afterwards the reader decides.
	let expanded = $state(untrack(() => open));
	const who = $derived(message.is_outgoing ? 'You' : displayName(message.from.name, message.from.address));
	const recipients = $derived([...message.to, ...message.cc].map((a) => a.name || a.address).join(', '));

	// Previews open the file's content, for Office files in LibreOffice on the server. That is
	// reserved for your own mail and senders you let in; everyone else's files are download-only.
	const trusted = $derived(
		message.is_outgoing || message.sender_category === 'important' || message.sender_category === 'feed'
	);
	const files = $derived<FileEntry[]>(
		message.attachments.map((a) => ({
			message_id: message.id,
			idx: a.idx,
			filename: a.filename,
			size: a.size,
			kind: a.kind,
			date: message.date,
			thread_id: message.thread_id,
			subject: message.subject,
			from_name: message.from.name,
			from_addr: message.from.address
		}))
	);
	let viewing = $state<number | null>(null);

	async function draft(kind: string) {
		goto(await startDraft(kind, message.id));
	}
</script>

<article class="sheet" class:collapsed={!expanded}>
	<button class="head" onclick={() => (expanded = !expanded)} aria-expanded={expanded}>
		<Avatar name={who} seed={message.from.address} size={38} address={message.from.address} />
		<span class="meta">
			<span class="who" title={message.from.address}>{who}</span>
			{#if expanded}
				<span class="to">to {recipients || 'undisclosed recipients'}</span>
			{:else}
				<span class="to">{message.body_text.slice(0, 140)}</span>
			{/if}
		</span>
		<time title={fullDate(message.date)}>{shortDate(message.date)}</time>
	</button>

	{#if expanded}
		<div class="body">
			{#if message.from.address}
				<p class="address">
					<span>From {message.from.address}</span>
					<button class="btn small quiet" onclick={() => copyText(message.from.address)}>Copy address</button>
				</p>
			{/if}
			{#if showSubject}
				<h2><a href="/thread/{message.thread_id}">{message.subject || '(no subject)'}</a></h2>
			{/if}
			<MessageBody {message} />
			{#if files.length && trusted}
				<div class="previews">
					{#each files as file, index (file.idx)}
						<FileTile
							{file}
							officePreviews={app.officePreviews}
							caption={fileSize(file.size)}
							onopen={() => (viewing = index)}
						/>
					{/each}
				</div>
			{:else if files.length}
				<ul class="files">
					{#each message.attachments as file}
						<li>
							<a class="btn small" href="/api/messages/{message.id}/attachments/{file.idx}" download={file.filename}>
								{file.filename} <span class="muted">{fileSize(file.size)}</span>
							</a>
						</li>
					{/each}
				</ul>
			{/if}
			{#if actions}
				<div class="actions">
					<button class="btn small" onclick={() => draft('reply')}>Reply</button>
					{#if message.to.length + message.cc.length > 1}
						<button class="btn small" onclick={() => draft('reply_all')}>Reply all</button>
					{/if}
					<button class="btn small" onclick={() => draft('forward')}>Forward</button>
				</div>
			{/if}
		</div>
	{/if}
</article>

{#if viewing !== null}
	<FileViewer
		{files}
		bind:index={viewing}
		officePreviews={app.officePreviews}
		emailLink={false}
		onclose={() => (viewing = null)}
	/>
{/if}

<style>
	article {
		margin-bottom: 0.75rem;
	}
	.head {
		display: flex;
		gap: 0.75rem;
		align-items: center;
		width: 100%;
		padding: 0.8rem 1rem;
		border: 0;
		background: none;
		text-align: left;
		cursor: pointer;
	}
	.meta {
		display: grid;
		min-width: 0;
		flex: 1;
	}
	.who {
		font-weight: 700;
	}
	.to {
		font-size: 0.875rem;
		color: var(--ink-soft);
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	time {
		flex: none;
		font-size: 0.85rem;
		color: var(--ink-soft);
	}
	.body {
		padding: 0 1rem 1rem;
	}
	.address {
		display: flex;
		align-items: center;
		flex-wrap: wrap;
		gap: 0.25rem 0.5rem;
		margin: -0.35rem 0 0.75rem;
		font-size: 0.875rem;
		color: var(--ink-soft);
	}
	.address span {
		overflow-wrap: anywhere;
		user-select: all;
	}
	h2 {
		margin-bottom: 0.75rem;
	}
	h2 a {
		text-decoration: none;
	}
	h2 a:hover {
		text-decoration: underline;
	}
	.previews {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(9rem, 1fr));
		gap: 1rem;
		margin-top: 1rem;
	}
	.files {
		list-style: none;
		display: flex;
		flex-wrap: wrap;
		gap: 0.5rem;
		padding: 0;
		margin: 1rem 0 0;
	}
	.actions {
		display: flex;
		gap: 0.5rem;
		margin-top: 1rem;
		padding-top: 0.9rem;
		border-top: 1px solid var(--line);
	}
</style>
