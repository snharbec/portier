-- Mail you send to yourself arrives in the inbox as well. Those copies used to be stored as sent
-- mail and so showed only in Sent; they are received mail and belong in the lists.
INSERT INTO senders (user_id, address, display_name, category, decided_at)
SELECT DISTINCT a.user_id, lower(a.address), a.display_name, 'important', unixepoch()
FROM accounts a
WHERE EXISTS (
    SELECT 1 FROM messages m JOIN folders f ON f.id = m.folder_id
    WHERE m.user_id = a.user_id AND f.role = 'inbox' AND lower(m.from_addr) = lower(a.address)
)
ON CONFLICT (user_id, address) DO UPDATE SET
    category = COALESCE(category, 'important'),
    decided_at = COALESCE(decided_at, unixepoch());

UPDATE messages
SET is_outgoing = 0,
    sender_id = (SELECT s.id FROM senders s WHERE s.user_id = messages.user_id AND s.address = lower(messages.from_addr))
WHERE is_outgoing = 1
  AND folder_id IN (SELECT id FROM folders WHERE role = 'inbox')
  AND lower(from_addr) IN (SELECT lower(address) FROM accounts WHERE user_id = messages.user_id);
