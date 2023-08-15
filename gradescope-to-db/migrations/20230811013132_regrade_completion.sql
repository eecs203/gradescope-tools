-- Add migration script here
ALTER TABLE regrade
ADD completed TINYINT NOT NULL DEFAULT false;