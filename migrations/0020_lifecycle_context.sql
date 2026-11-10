ALTER TABLE feeding_presentation ADD COLUMN override_enabled INTEGER CHECK(override_enabled IN (0,1));
CREATE TABLE solar_presentation(singleton INTEGER PRIMARY KEY CHECK(singleton=1),document_json TEXT NOT NULL);
