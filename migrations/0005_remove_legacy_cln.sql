PRAGMA foreign_keys = ON;

-- Phase 1 production accounting is provider-neutral and Strike-backed.
-- These tables existed only for the retired CLN/clnaddress/LNbits transition.
-- Keep the historical migrations immutable so existing development databases
-- can upgrade safely, then remove the obsolete schema here.
DROP TABLE IF EXISTS cln_cursor;
DROP TABLE IF EXISTS settled_invoices;
DROP TABLE IF EXISTS legacy_imports;
