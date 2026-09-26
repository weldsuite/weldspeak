-- Where a dictionary term came from.
--
--   manual      typed in by a person (dashboard or Hub)
--   correction  learned from a person fixing a misheard word after a dictation
--   harvested   picked out of a dictation's own output by older desktop builds
--
-- Harvesting read every capitalised or hyphenated word in the text WeldSpeak
-- had just typed ("Maybe", "Honestly", "follow-up") and added it to the
-- dictionary, misheard words included. Those were then boosted as keyterms on
-- the next dictation, so recognition got worse the more someone used it.
-- Harvested rows are kept, not deleted, but take no part in dictation and are
-- hidden from the dictionary; adding the same term by hand revives it.
-- Existing rows start as 'manual'; only harvested ones can be told apart.
ALTER TABLE dictionary_terms
  ADD COLUMN source TEXT NOT NULL DEFAULT 'manual'
  CHECK (source IN ('manual', 'correction', 'harvested'));

-- Harvested rows were uploaded seconds after the dictation they came from was
-- stored, and contain a word of it. A term someone typed in by hand does not
-- line up with one of their own dictations like that.
UPDATE dictionary_terms
   SET source = 'harvested'
 WHERE scope = 'user'
   AND sounds_like IS NULL
   AND EXISTS (
     SELECT 1
       FROM transcripts t
      WHERE t.clerk_user_id = dictionary_terms.clerk_user_id
        AND instr(lower(t.formatted), lower(dictionary_terms.term)) > 0
        AND abs(julianday(t.created_at) - julianday(dictionary_terms.created_at)) * 86400 <= 600
   );
