import { api } from './api.ts';
import { app, notify, refreshCounts } from './app.svelte.ts';

export type SwipeAction = 'read' | 'archive' | 'move' | 'trash';

/** Every action a slide can be given, in the order they are offered. */
export const swipeActions: { id: SwipeAction; name: string }[] = [
	{ id: 'read', name: 'Mark as read or unread' },
	{ id: 'archive', name: 'Archive' },
	{ id: 'move', name: 'Move to folder' },
	{ id: 'trash', name: 'Move to Trash' }
];

export interface Target {
	threadIds?: number[];
	messageIds?: number[];
}

/** Runs one of the server's mail actions, says what happened and reloads the lists. */
export async function mailAction(
	action: 'read' | 'unread' | 'archive' | 'trash' | 'move',
	target: Target,
	done: string,
	extra: Record<string, unknown> = {}
) {
	await api.post('/mail/actions', {
		action,
		thread_ids: target.threadIds ?? [],
		message_ids: target.messageIds ?? [],
		...extra
	});
	notify(done);
	app.tick += 1;
	refreshCounts().catch(() => {});
}

export type FolderChoice = { accountId: number; folder: string } | null;

/** What a slide does to one row. `chooseFolder` opens the folder picker. */
export async function swipeMail(
	action: SwipeAction,
	mail: Target & { accountId: number; unread: boolean; what: string },
	chooseFolder: (accountIds: number[], what: string) => Promise<FolderChoice>
) {
	try {
		if (action === 'read') {
			if (mail.unread) await mailAction('read', mail, `Marked as read: ${mail.what}`);
			else await mailAction('unread', mail, `Marked as unread: ${mail.what}`);
		} else if (action === 'archive') {
			await mailAction('archive', mail, `Archived: ${mail.what}`);
		} else if (action === 'trash') {
			await mailAction('trash', mail, `Moved to Trash: ${mail.what}`);
		} else {
			const choice = await chooseFolder([mail.accountId], mail.what);
			if (choice) {
				await mailAction('move', mail, `Moved to ${choice.folder}: ${mail.what}`, {
					account_id: choice.accountId,
					folder: choice.folder
				});
			}
		}
	} catch (e) {
		notify((e as Error).message);
	}
}

export function swipeLabel(action: SwipeAction, unread: boolean): string {
	if (action === 'read') return unread ? 'Mark as read' : 'Mark as unread';
	if (action === 'archive') return 'Archive';
	return action === 'trash' ? 'Move to Trash' : 'Move to folder';
}
