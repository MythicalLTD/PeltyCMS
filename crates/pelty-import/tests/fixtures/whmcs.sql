-- MySQL dump 10.13  Distrib 8.0.36, for Linux (x86_64)
--
-- Host: localhost    Database: whmcs
-- ------------------------------------------------------
-- Server version	8.0.36
--
-- Synthetic WHMCS 8.10 export used by PeltyCMS tests (tests/sqldump_fixture.rs).
-- @T@ is replaced by a unique tag per test run. cc_encryption_hash: Xk2v9QpL7mZr4tYw1sN8cB3dF6gH0jKa
-- Card columns / tblcreditcards hold fake values: the importer must never read them.

/*!40101 SET @OLD_CHARACTER_SET_CLIENT=@@CHARACTER_SET_CLIENT */;
/*!50503 SET NAMES utf8mb4 */;
/*!40014 SET @OLD_UNIQUE_CHECKS=@@UNIQUE_CHECKS, UNIQUE_CHECKS=0 */;

--
-- Table structure for table `tblconfiguration`
--

DROP TABLE IF EXISTS `tblconfiguration`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
CREATE TABLE `tblconfiguration` (
  `setting` varchar(64) NOT NULL DEFAULT '',
  `value` text DEFAULT NULL,
  KEY `setting` (`setting`(32))
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Dumping data for table `tblconfiguration`
--

LOCK TABLES `tblconfiguration` WRITE;
/*!40000 ALTER TABLE `tblconfiguration` DISABLE KEYS */;
INSERT INTO `tblconfiguration` VALUES ('CompanyName','Fixture Hosting @T@'),('Version','8.10.1-release.1');
/*!40000 ALTER TABLE `tblconfiguration` ENABLE KEYS */;
UNLOCK TABLES;

--
-- Table structure for table `tblcurrencies`
--

DROP TABLE IF EXISTS `tblcurrencies`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
CREATE TABLE `tblcurrencies` (
  `id` int(10) unsigned NOT NULL AUTO_INCREMENT,
  `code` text DEFAULT NULL,
  `prefix` text DEFAULT NULL,
  `suffix` text DEFAULT NULL,
  `format` int(1) NOT NULL DEFAULT '0',
  `rate` decimal(10,5) NOT NULL DEFAULT '',
  `default` int(1) NOT NULL DEFAULT '0',
  PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Dumping data for table `tblcurrencies`
--

LOCK TABLES `tblcurrencies` WRITE;
/*!40000 ALTER TABLE `tblcurrencies` DISABLE KEYS */;
INSERT INTO `tblcurrencies` VALUES (1,'USD','$',' USD',1,'1.00000',1),(2,'EUR','€',' EUR',3,'0.92000',0);
/*!40000 ALTER TABLE `tblcurrencies` ENABLE KEYS */;
UNLOCK TABLES;

--
-- Table structure for table `tblclientgroups`
--

DROP TABLE IF EXISTS `tblclientgroups`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
CREATE TABLE `tblclientgroups` (
  `id` int(10) unsigned NOT NULL AUTO_INCREMENT,
  `groupname` varchar(45) NOT NULL DEFAULT '',
  `groupcolour` varchar(45) NOT NULL DEFAULT '',
  `discountpercent` decimal(10,2) NOT NULL DEFAULT '',
  `susptermexempt` text DEFAULT NULL,
  `separateinvoices` text DEFAULT NULL,
  PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Dumping data for table `tblclientgroups`
--

LOCK TABLES `tblclientgroups` WRITE;
/*!40000 ALTER TABLE `tblclientgroups` DISABLE KEYS */;
INSERT INTO `tblclientgroups` VALUES (1,'Resellers @T@','#ff9900','15.00','on','');
/*!40000 ALTER TABLE `tblclientgroups` ENABLE KEYS */;
UNLOCK TABLES;

--
-- Table structure for table `tblclients`
--

DROP TABLE IF EXISTS `tblclients`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
CREATE TABLE `tblclients` (
  `id` int(10) unsigned NOT NULL AUTO_INCREMENT,
  `uuid` varchar(255) NOT NULL DEFAULT '',
  `firstname` text DEFAULT NULL,
  `lastname` text DEFAULT NULL,
  `companyname` text DEFAULT NULL,
  `email` text DEFAULT NULL,
  `address1` text DEFAULT NULL,
  `address2` text DEFAULT NULL,
  `city` text DEFAULT NULL,
  `state` text DEFAULT NULL,
  `postcode` text DEFAULT NULL,
  `country` text DEFAULT NULL,
  `phonenumber` text DEFAULT NULL,
  `tax_id` varchar(128) NOT NULL DEFAULT '',
  `password` text DEFAULT NULL,
  `authmodule` text DEFAULT NULL,
  `currency` int(10) NOT NULL DEFAULT '0',
  `defaultgateway` text DEFAULT NULL,
  `credit` decimal(16,2) NOT NULL DEFAULT '',
  `taxexempt` tinyint(1) NOT NULL DEFAULT '0',
  `groupid` int(10) NOT NULL DEFAULT '0',
  `cardtype` varchar(255) NOT NULL DEFAULT '',
  `cardlastfour` text DEFAULT NULL,
  `cardnum` blob DEFAULT NULL,
  `startdate` blob DEFAULT NULL,
  `expdate` blob DEFAULT NULL,
  `issuenumber` blob DEFAULT NULL,
  `bankname` text DEFAULT NULL,
  `banktype` text DEFAULT NULL,
  `bankcode` blob DEFAULT NULL,
  `bankacct` blob DEFAULT NULL,
  `datecreated` date DEFAULT NULL,
  `notes` text DEFAULT NULL,
  `status` enum('Active','Inactive','Closed') DEFAULT NULL,
  `language` text DEFAULT NULL,
  `created_at` timestamp DEFAULT NULL,
  `updated_at` timestamp DEFAULT NULL,
  PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Dumping data for table `tblclients`
--

LOCK TABLES `tblclients` WRITE;
/*!40000 ALTER TABLE `tblclients` DISABLE KEYS */;
INSERT INTO `tblclients` VALUES (1,'7d3c0f6e-1111-4b8e-9b55-000000000001','Ana','Popescu','','ana-@T@@example.com','Str. Lalelelor 12','Ap. 4','Cluj-Napoca','Cluj','400001','RO','+40.744123456','RO12345678','','',1,'stripe','25.00',0,0,'Visa','4242',0x9F2C11AB,'',0xAB01,'','','','','','2023-05-14','Prefers email','Active','romanian','2023-05-14 10:00:00','2024-02-01 09:00:00'),(2,'7d3c0f6e-1111-4b8e-9b55-000000000002','Bob','O\'Brien','O\'Brien \"Web\" Ltd','bob-@T@@example.org','1 High St','','Dublin','','D02','IE','+353.1234567','','d85860ab87f9b0dd4ba592810381a625:a1#Zx','',2,'paypal','0.00',0,1,'','',NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,'2015-03-02','Legacy client\nsince 2015','Active','','2015-03-02 00:00:00','2015-03-02 00:00:00'),(3,'7d3c0f6e-1111-4b8e-9b55-000000000003','Carla','Weiß','Weiß GmbH','carla-@T@@example.de','Hauptstr. 5','','Wien','','1010','AT','','ATU12345678','sha256-unknown-format','',1,'banktransfer','0.00',0,0,'MasterCard','5555',0x00FF,'','','','Erste','Checking',0x0102,0x0304,'2020-01-01','','Inactive','german','2020-01-01 00:00:00','2020-01-01 00:00:00');
/*!40000 ALTER TABLE `tblclients` ENABLE KEYS */;
UNLOCK TABLES;

--
-- Table structure for table `tblusers`
--

DROP TABLE IF EXISTS `tblusers`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
CREATE TABLE `tblusers` (
  `id` int(10) unsigned NOT NULL AUTO_INCREMENT,
  `first_name` varchar(255) NOT NULL DEFAULT '',
  `last_name` varchar(255) NOT NULL DEFAULT '',
  `email` varchar(255) NOT NULL DEFAULT '',
  `password` varchar(255) NOT NULL DEFAULT '',
  `second_factor` varchar(255) NOT NULL DEFAULT '',
  `email_verified_at` timestamp DEFAULT NULL,
  `created_at` timestamp DEFAULT NULL,
  PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Dumping data for table `tblusers`
--

LOCK TABLES `tblusers` WRITE;
/*!40000 ALTER TABLE `tblusers` DISABLE KEYS */;
INSERT INTO `tblusers` VALUES (1,'Ana','Popescu','ANA-@T@@example.com','$2y$10$9b2PCEp5BOppVq8RTzpBvuOmSPQ3gmS68ujZpY4jniH9lqge5a1im','',NULL,'2023-05-14 10:00:00');
/*!40000 ALTER TABLE `tblusers` ENABLE KEYS */;
UNLOCK TABLES;

--
-- Table structure for table `tblcreditcards`
--

DROP TABLE IF EXISTS `tblcreditcards`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
CREATE TABLE `tblcreditcards` (
  `id` int(10) unsigned NOT NULL AUTO_INCREMENT,
  `pay_method_id` int(10) NOT NULL DEFAULT '0',
  `card_type` varchar(255) NOT NULL DEFAULT '',
  `last_four` varchar(255) NOT NULL DEFAULT '',
  `expiry_date` timestamp DEFAULT NULL,
  `card_data` blob DEFAULT NULL,
  PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Dumping data for table `tblcreditcards`
--

LOCK TABLES `tblcreditcards` WRITE;
/*!40000 ALTER TABLE `tblcreditcards` DISABLE KEYS */;
INSERT INTO `tblcreditcards` VALUES (1,1,'Visa','4242','2027-12-01 00:00:00',0xDEADBEEF4111111111111111);
/*!40000 ALTER TABLE `tblcreditcards` ENABLE KEYS */;
UNLOCK TABLES;

--
-- Table structure for table `tblcontacts`
--

DROP TABLE IF EXISTS `tblcontacts`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
CREATE TABLE `tblcontacts` (
  `id` int(10) unsigned NOT NULL AUTO_INCREMENT,
  `userid` int(10) NOT NULL DEFAULT '0',
  `firstname` text DEFAULT NULL,
  `lastname` text DEFAULT NULL,
  `companyname` text DEFAULT NULL,
  `email` text DEFAULT NULL,
  `address1` text DEFAULT NULL,
  `city` text DEFAULT NULL,
  `country` text DEFAULT NULL,
  `phonenumber` text DEFAULT NULL,
  `password` text DEFAULT NULL,
  `generalemails` int(1) NOT NULL DEFAULT '0',
  `invoiceemails` int(1) NOT NULL DEFAULT '0',
  `supportemails` int(1) NOT NULL DEFAULT '0',
  PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Dumping data for table `tblcontacts`
--

LOCK TABLES `tblcontacts` WRITE;
/*!40000 ALTER TABLE `tblcontacts` DISABLE KEYS */;
INSERT INTO `tblcontacts` VALUES (1,1,'Mihai','Popescu','','billing-@T@@example.com','Str. Lalelelor 12','Cluj-Napoca','RO','+40.744000111','',0,1,0),(2,2,'Tech','Contact','','tech-@T@@example.org','1 High St','Dublin','IE','','',1,0,1);
/*!40000 ALTER TABLE `tblcontacts` ENABLE KEYS */;
UNLOCK TABLES;

--
-- Table structure for table `tblproductgroups`
--

DROP TABLE IF EXISTS `tblproductgroups`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
CREATE TABLE `tblproductgroups` (
  `id` int(10) unsigned NOT NULL AUTO_INCREMENT,
  `name` text DEFAULT NULL,
  `slug` varchar(128) NOT NULL DEFAULT '',
  `headline` text DEFAULT NULL,
  `tagline` text DEFAULT NULL,
  `orderfrmtpl` text DEFAULT NULL,
  `disabledgateways` text DEFAULT NULL,
  `hidden` tinyint(1) NOT NULL DEFAULT '0',
  `order` int(1) NOT NULL DEFAULT '0',
  PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Dumping data for table `tblproductgroups`
--

LOCK TABLES `tblproductgroups` WRITE;
/*!40000 ALTER TABLE `tblproductgroups` DISABLE KEYS */;
INSERT INTO `tblproductgroups` VALUES (1,'Shared Hosting @T@','shared-@T@','Fast & simple','','','',0,1),(2,'VPS @T@','vps-@T@','','','','',1,2);
/*!40000 ALTER TABLE `tblproductgroups` ENABLE KEYS */;
UNLOCK TABLES;

--
-- Table structure for table `tblproducts`
--

DROP TABLE IF EXISTS `tblproducts`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
CREATE TABLE `tblproducts` (
  `id` int(10) unsigned NOT NULL AUTO_INCREMENT,
  `type` text DEFAULT NULL,
  `gid` int(10) NOT NULL DEFAULT '0',
  `name` text DEFAULT NULL,
  `slug` varchar(128) NOT NULL DEFAULT '',
  `description` text DEFAULT NULL,
  `hidden` tinyint(1) NOT NULL DEFAULT '0',
  `showdomainoptions` tinyint(1) NOT NULL DEFAULT '0',
  `paytype` text DEFAULT NULL,
  `servertype` text DEFAULT NULL,
  `servergroup` int(10) NOT NULL DEFAULT '0',
  `configoption1` text DEFAULT NULL,
  `configoption2` text DEFAULT NULL,
  `retired` tinyint(1) NOT NULL DEFAULT '0',
  `order` int(10) NOT NULL DEFAULT '0',
  PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Dumping data for table `tblproducts`
--

LOCK TABLES `tblproducts` WRITE;
/*!40000 ALTER TABLE `tblproducts` DISABLE KEYS */;
INSERT INTO `tblproducts` VALUES (1,'hostingaccount',1,'Starter @T@','starter-@T@','<p>1 site, 10 GB SSD</p>',0,1,'recurring','cpanel',1,'starter_pkg','',0,1),(2,'hostingaccount',1,'Pro @T@','pro-@T@','Unlimited sites',0,1,'recurring','cpanel',1,'pro_pkg','',0,2),(3,'server',2,'VPS-2G @T@','vps2g-@T@','2 GB RAM VPS',0,0,'recurring','virtualizor',2,'','',0,1),(4,'other',1,'Old Plan @T@','old-@T@','',1,0,'onetime','',0,'','',1,9);
/*!40000 ALTER TABLE `tblproducts` ENABLE KEYS */;
UNLOCK TABLES;

--
-- Table structure for table `tblpricing`
--

DROP TABLE IF EXISTS `tblpricing`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
CREATE TABLE `tblpricing` (
  `id` int(10) unsigned NOT NULL AUTO_INCREMENT,
  `type` enum('product','addon','configoptions','domainregister') DEFAULT NULL,
  `currency` int(10) NOT NULL DEFAULT '0',
  `relid` int(10) NOT NULL DEFAULT '0',
  `msetupfee` decimal(16,2) NOT NULL DEFAULT '',
  `qsetupfee` decimal(16,2) NOT NULL DEFAULT '',
  `ssetupfee` decimal(16,2) NOT NULL DEFAULT '',
  `asetupfee` decimal(16,2) NOT NULL DEFAULT '',
  `bsetupfee` decimal(16,2) NOT NULL DEFAULT '',
  `tsetupfee` decimal(16,2) NOT NULL DEFAULT '',
  `monthly` decimal(16,2) NOT NULL DEFAULT '',
  `quarterly` decimal(16,2) NOT NULL DEFAULT '',
  `semiannually` decimal(16,2) NOT NULL DEFAULT '',
  `annually` decimal(16,2) NOT NULL DEFAULT '',
  `biennially` decimal(16,2) NOT NULL DEFAULT '',
  `triennially` decimal(16,2) NOT NULL DEFAULT '',
  PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Dumping data for table `tblpricing`
--

LOCK TABLES `tblpricing` WRITE;
/*!40000 ALTER TABLE `tblpricing` DISABLE KEYS */;
INSERT INTO `tblpricing` VALUES (1,'product',1,1,'0.00','0.00','0.00','0.00','0.00','0.00','4.99','14.50','28.00','49.90','-1.00','-1.00'),(2,'product',2,1,'0.00','0.00','0.00','0.00','0.00','0.00','4.59','-1.00','-1.00','45.90','-1.00','-1.00'),(3,'product',1,2,'10.00','0.00','0.00','0.00','0.00','0.00','9.99','-1.00','-1.00','99.00','180.00','-1.00'),(4,'product',1,3,'0.00','0.00','0.00','0.00','0.00','0.00','19.00','-1.00','-1.00','-1.00','-1.00','-1.00'),(5,'product',1,4,'25.00','-1.00','-1.00','-1.00','-1.00','-1.00','0.00','-1.00','-1.00','-1.00','-1.00','-1.00'),(6,'addon',1,1,'0.00','0.00','0.00','0.00','0.00','0.00','2.00','-1.00','-1.00','20.00','-1.00','-1.00'),(7,'configoptions',1,2,'0.00','0.00','0.00','0.00','0.00','0.00','5.00','-1.00','-1.00','50.00','-1.00','-1.00'),(8,'domainregister',1,1,'0.00','0.00','0.00','0.00','0.00','0.00','9.99','19.98','-1.00','-1.00','-1.00','-1.00');
/*!40000 ALTER TABLE `tblpricing` ENABLE KEYS */;
UNLOCK TABLES;

--
-- Table structure for table `tblproductconfiggroups`
--

DROP TABLE IF EXISTS `tblproductconfiggroups`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
CREATE TABLE `tblproductconfiggroups` (
  `id` int(10) unsigned NOT NULL AUTO_INCREMENT,
  `name` text DEFAULT NULL,
  `description` text DEFAULT NULL,
  PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Dumping data for table `tblproductconfiggroups`
--

LOCK TABLES `tblproductconfiggroups` WRITE;
/*!40000 ALTER TABLE `tblproductconfiggroups` DISABLE KEYS */;
INSERT INTO `tblproductconfiggroups` VALUES (1,'VPS options @T@','');
/*!40000 ALTER TABLE `tblproductconfiggroups` ENABLE KEYS */;
UNLOCK TABLES;

--
-- Table structure for table `tblproductconfiglinks`
--

DROP TABLE IF EXISTS `tblproductconfiglinks`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
CREATE TABLE `tblproductconfiglinks` (
  `id` int(10) unsigned NOT NULL AUTO_INCREMENT,
  `gid` int(10) NOT NULL DEFAULT '0',
  `pid` int(10) NOT NULL DEFAULT '0',
  PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Dumping data for table `tblproductconfiglinks`
--

LOCK TABLES `tblproductconfiglinks` WRITE;
/*!40000 ALTER TABLE `tblproductconfiglinks` DISABLE KEYS */;
INSERT INTO `tblproductconfiglinks` VALUES (1,1,3);
/*!40000 ALTER TABLE `tblproductconfiglinks` ENABLE KEYS */;
UNLOCK TABLES;

--
-- Table structure for table `tblproductconfigoptions`
--

DROP TABLE IF EXISTS `tblproductconfigoptions`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
CREATE TABLE `tblproductconfigoptions` (
  `id` int(10) unsigned NOT NULL AUTO_INCREMENT,
  `gid` int(10) NOT NULL DEFAULT '0',
  `optionname` text DEFAULT NULL,
  `optiontype` text DEFAULT NULL,
  `qtyminimum` int(10) NOT NULL DEFAULT '0',
  `qtymaximum` int(10) NOT NULL DEFAULT '0',
  `order` int(1) NOT NULL DEFAULT '0',
  `hidden` tinyint(1) NOT NULL DEFAULT '0',
  PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Dumping data for table `tblproductconfigoptions`
--

LOCK TABLES `tblproductconfigoptions` WRITE;
/*!40000 ALTER TABLE `tblproductconfigoptions` DISABLE KEYS */;
INSERT INTO `tblproductconfigoptions` VALUES (1,1,'ram|Memory','1',0,0,1,0);
/*!40000 ALTER TABLE `tblproductconfigoptions` ENABLE KEYS */;
UNLOCK TABLES;

--
-- Table structure for table `tblproductconfigoptionssub`
--

DROP TABLE IF EXISTS `tblproductconfigoptionssub`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
CREATE TABLE `tblproductconfigoptionssub` (
  `id` int(10) unsigned NOT NULL AUTO_INCREMENT,
  `configid` int(10) NOT NULL DEFAULT '0',
  `optionname` text DEFAULT NULL,
  `sortorder` int(10) NOT NULL DEFAULT '0',
  `hidden` tinyint(1) NOT NULL DEFAULT '0',
  PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Dumping data for table `tblproductconfigoptionssub`
--

LOCK TABLES `tblproductconfigoptionssub` WRITE;
/*!40000 ALTER TABLE `tblproductconfigoptionssub` DISABLE KEYS */;
INSERT INTO `tblproductconfigoptionssub` VALUES (1,1,'2048|2 GB',1,0),(2,1,'4096|4 GB',2,0);
/*!40000 ALTER TABLE `tblproductconfigoptionssub` ENABLE KEYS */;
UNLOCK TABLES;

--
-- Table structure for table `tbladdons`
--

DROP TABLE IF EXISTS `tbladdons`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
CREATE TABLE `tbladdons` (
  `id` int(10) unsigned NOT NULL AUTO_INCREMENT,
  `packages` text DEFAULT NULL,
  `name` text DEFAULT NULL,
  `description` text DEFAULT NULL,
  `billingcycle` text DEFAULT NULL,
  `tax` tinyint(1) NOT NULL DEFAULT '0',
  `showorder` tinyint(1) NOT NULL DEFAULT '0',
  `hidden` tinyint(1) NOT NULL DEFAULT '0',
  `retired` tinyint(1) NOT NULL DEFAULT '0',
  `weight` int(10) NOT NULL DEFAULT '0',
  PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Dumping data for table `tbladdons`
--

LOCK TABLES `tbladdons` WRITE;
/*!40000 ALTER TABLE `tbladdons` DISABLE KEYS */;
INSERT INTO `tbladdons` VALUES (1,'1,2','Daily Backups @T@','Off-site backups','recurring',1,1,0,0,1);
/*!40000 ALTER TABLE `tbladdons` ENABLE KEYS */;
UNLOCK TABLES;

--
-- Table structure for table `tblservers`
--

DROP TABLE IF EXISTS `tblservers`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
CREATE TABLE `tblservers` (
  `id` int(10) unsigned NOT NULL AUTO_INCREMENT,
  `name` text DEFAULT NULL,
  `ipaddress` text DEFAULT NULL,
  `assignedips` text DEFAULT NULL,
  `hostname` text DEFAULT NULL,
  `monthlycost` decimal(16,2) NOT NULL DEFAULT '',
  `noc` text DEFAULT NULL,
  `statusaddress` text DEFAULT NULL,
  `nameserver1` text DEFAULT NULL,
  `maxaccounts` int(10) NOT NULL DEFAULT '0',
  `type` text DEFAULT NULL,
  `username` text DEFAULT NULL,
  `password` text DEFAULT NULL,
  `accesshash` text DEFAULT NULL,
  `secure` text DEFAULT NULL,
  `port` int(8) NOT NULL DEFAULT '0',
  `active` int(1) NOT NULL DEFAULT '0',
  `disabled` int(1) NOT NULL DEFAULT '0',
  PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Dumping data for table `tblservers`
--

LOCK TABLES `tblservers` WRITE;
/*!40000 ALTER TABLE `tblservers` DISABLE KEYS */;
INSERT INTO `tblservers` VALUES (1,'WHM-01 @T@','203.0.113.10','','whm01.example.net','40.00','','','ns1.example.net',200,'cpanel','root','Ix9DrK1/zzSbPxWAYvQ3zf8mqMtTMjNwKFZmLC0nbWN/I3h4fD9gcfhLgJAww+8M','','on',2087,1,0),(2,'Virt-01 @T@','203.0.113.20','','','0.00','','','',0,'virtualizor','apikey','','VIRT-ACCESS-HASH-123','on',4085,1,0),(3,'Old box @T@','','','','0.00','','','',0,'cpanel','root','','','',0,0,1);
/*!40000 ALTER TABLE `tblservers` ENABLE KEYS */;
UNLOCK TABLES;

--
-- Table structure for table `tblhosting`
--

DROP TABLE IF EXISTS `tblhosting`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
CREATE TABLE `tblhosting` (
  `id` int(10) unsigned NOT NULL AUTO_INCREMENT,
  `userid` int(10) NOT NULL DEFAULT '0',
  `orderid` int(10) NOT NULL DEFAULT '0',
  `packageid` int(10) NOT NULL DEFAULT '0',
  `server` int(10) NOT NULL DEFAULT '0',
  `regdate` date DEFAULT NULL,
  `domain` text DEFAULT NULL,
  `paymentmethod` text DEFAULT NULL,
  `firstpaymentamount` decimal(16,2) NOT NULL DEFAULT '',
  `amount` decimal(16,2) NOT NULL DEFAULT '',
  `billingcycle` text DEFAULT NULL,
  `nextduedate` date DEFAULT NULL,
  `nextinvoicedate` date DEFAULT NULL,
  `domainstatus` enum('Pending','Active','Suspended','Terminated','Cancelled','Fraud','Completed') DEFAULT NULL,
  `username` text DEFAULT NULL,
  `password` text DEFAULT NULL,
  `notes` text DEFAULT NULL,
  `dedicatedip` text DEFAULT NULL,
  PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Dumping data for table `tblhosting`
--

LOCK TABLES `tblhosting` WRITE;
/*!40000 ALTER TABLE `tblhosting` DISABLE KEYS */;
INSERT INTO `tblhosting` VALUES (1,1,1,1,1,'2023-05-14','ana-@T@.ro','stripe','4.99','4.99','Monthly','2024-03-14','2024-03-14','Active','anapop','Ih1AqKh5yDySNR6Mb/o43e40u99hY2N0LXBhc3M=','',''),(2,2,2,2,1,'2015-03-02','obrien-@T@.ie','paypal','120.00','99.00','Annually','2025-03-02','2025-03-02','Suspended','obrienw','','overdue',''),(3,1,3,3,2,'2024-01-10','vps-@T@.example','stripe','19.00','24.00','Monthly','2024-02-10','2024-02-10','Active','','','','203.0.113.50'),(4,3,4,4,0,'2020-01-01','','banktransfer','25.00','0.00','Free Account','0000-00-00','0000-00-00','Terminated','','','','');
/*!40000 ALTER TABLE `tblhosting` ENABLE KEYS */;
UNLOCK TABLES;

--
-- Table structure for table `tblhostingaddons`
--

DROP TABLE IF EXISTS `tblhostingaddons`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
CREATE TABLE `tblhostingaddons` (
  `id` int(10) unsigned NOT NULL AUTO_INCREMENT,
  `orderid` int(10) NOT NULL DEFAULT '0',
  `hostingid` int(10) NOT NULL DEFAULT '0',
  `addonid` int(10) NOT NULL DEFAULT '0',
  `name` text DEFAULT NULL,
  `setupfee` decimal(16,2) NOT NULL DEFAULT '',
  `recurring` decimal(16,2) NOT NULL DEFAULT '',
  `billingcycle` text DEFAULT NULL,
  `status` enum('Pending','Active','Suspended','Terminated','Cancelled','Fraud','Completed') DEFAULT NULL,
  `regdate` date DEFAULT NULL,
  `nextduedate` date DEFAULT NULL,
  PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Dumping data for table `tblhostingaddons`
--

LOCK TABLES `tblhostingaddons` WRITE;
/*!40000 ALTER TABLE `tblhostingaddons` DISABLE KEYS */;
INSERT INTO `tblhostingaddons` VALUES (1,1,1,1,'','0.00','2.00','Monthly','Active','2023-05-14','2024-03-14');
/*!40000 ALTER TABLE `tblhostingaddons` ENABLE KEYS */;
UNLOCK TABLES;

--
-- Table structure for table `tblhostingconfigoptions`
--

DROP TABLE IF EXISTS `tblhostingconfigoptions`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
CREATE TABLE `tblhostingconfigoptions` (
  `id` int(10) unsigned NOT NULL AUTO_INCREMENT,
  `relid` int(10) NOT NULL DEFAULT '0',
  `configid` int(10) NOT NULL DEFAULT '0',
  `optionid` int(10) NOT NULL DEFAULT '0',
  `qty` int(10) NOT NULL DEFAULT '0',
  PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Dumping data for table `tblhostingconfigoptions`
--

LOCK TABLES `tblhostingconfigoptions` WRITE;
/*!40000 ALTER TABLE `tblhostingconfigoptions` DISABLE KEYS */;
INSERT INTO `tblhostingconfigoptions` VALUES (1,3,1,2,0);
/*!40000 ALTER TABLE `tblhostingconfigoptions` ENABLE KEYS */;
UNLOCK TABLES;

--
-- Table structure for table `tblcustomfields`
--

DROP TABLE IF EXISTS `tblcustomfields`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
CREATE TABLE `tblcustomfields` (
  `id` int(10) unsigned NOT NULL AUTO_INCREMENT,
  `type` text DEFAULT NULL,
  `relid` int(10) NOT NULL DEFAULT '0',
  `fieldname` text DEFAULT NULL,
  `fieldtype` text DEFAULT NULL,
  `description` text DEFAULT NULL,
  `fieldoptions` text DEFAULT NULL,
  `regexpr` text DEFAULT NULL,
  `adminonly` text DEFAULT NULL,
  `required` text DEFAULT NULL,
  `showorder` text DEFAULT NULL,
  `showinvoice` text DEFAULT NULL,
  `sortorder` int(10) NOT NULL DEFAULT '0',
  PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Dumping data for table `tblcustomfields`
--

LOCK TABLES `tblcustomfields` WRITE;
/*!40000 ALTER TABLE `tblcustomfields` DISABLE KEYS */;
INSERT INTO `tblcustomfields` VALUES (1,'product',1,'Website purpose','dropdown','','Blog,Shop,Other','','','on','on','',0);
/*!40000 ALTER TABLE `tblcustomfields` ENABLE KEYS */;
UNLOCK TABLES;

--
-- Table structure for table `tblcustomfieldsvalues`
--

DROP TABLE IF EXISTS `tblcustomfieldsvalues`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
CREATE TABLE `tblcustomfieldsvalues` (
  `id` int(10) unsigned NOT NULL AUTO_INCREMENT,
  `fieldid` int(10) NOT NULL DEFAULT '0',
  `relid` int(10) NOT NULL DEFAULT '0',
  `value` text DEFAULT NULL,
  PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Dumping data for table `tblcustomfieldsvalues`
--

LOCK TABLES `tblcustomfieldsvalues` WRITE;
/*!40000 ALTER TABLE `tblcustomfieldsvalues` DISABLE KEYS */;
INSERT INTO `tblcustomfieldsvalues` VALUES (1,1,1,'Shop');
/*!40000 ALTER TABLE `tblcustomfieldsvalues` ENABLE KEYS */;
UNLOCK TABLES;

--
-- Table structure for table `tbldomains`
--

DROP TABLE IF EXISTS `tbldomains`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
CREATE TABLE `tbldomains` (
  `id` int(10) unsigned NOT NULL AUTO_INCREMENT,
  `userid` int(10) NOT NULL DEFAULT '0',
  `orderid` int(10) NOT NULL DEFAULT '0',
  `type` enum('Register','Transfer') DEFAULT NULL,
  `registrationdate` date DEFAULT NULL,
  `domain` text DEFAULT NULL,
  `firstpaymentamount` decimal(16,2) NOT NULL DEFAULT '',
  `recurringamount` decimal(16,2) NOT NULL DEFAULT '',
  `registrar` text DEFAULT NULL,
  `registrationperiod` int(1) NOT NULL DEFAULT '0',
  `expirydate` date DEFAULT NULL,
  `subscriptionid` text DEFAULT NULL,
  `status` enum('Pending','Pending Registration','Pending Transfer','Active','Grace','Redemption','Expired','Cancelled','Fraud','Transferred Away') DEFAULT NULL,
  `nextduedate` date DEFAULT NULL,
  `nextinvoicedate` date DEFAULT NULL,
  `dnsmanagement` text DEFAULT NULL,
  `emailforwarding` text DEFAULT NULL,
  `idprotection` text DEFAULT NULL,
  `donotrenew` text DEFAULT NULL,
  PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Dumping data for table `tbldomains`
--

LOCK TABLES `tbldomains` WRITE;
/*!40000 ALTER TABLE `tbldomains` DISABLE KEYS */;
INSERT INTO `tbldomains` VALUES (1,1,1,'Register','2023-05-14','ana-@T@.ro','9.99','9.99','rotld',1,'2025-05-14','','Active','2025-05-14','2025-04-30','on','','',''),(2,2,2,'Transfer','2015-03-02','obrien-@T@.ie','0.00','19.98','',2,'0000-00-00','','Pending Transfer','2025-03-02','2025-03-02','','','on','on');
/*!40000 ALTER TABLE `tbldomains` ENABLE KEYS */;
UNLOCK TABLES;

--
-- Table structure for table `tblinvoices`
--

DROP TABLE IF EXISTS `tblinvoices`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
CREATE TABLE `tblinvoices` (
  `id` int(10) unsigned NOT NULL AUTO_INCREMENT,
  `userid` int(10) NOT NULL DEFAULT '0',
  `invoicenum` text DEFAULT NULL,
  `date` date DEFAULT NULL,
  `duedate` date DEFAULT NULL,
  `datepaid` datetime DEFAULT NULL,
  `last_capture_attempt` datetime DEFAULT NULL,
  `subtotal` decimal(16,2) NOT NULL DEFAULT '',
  `credit` decimal(16,2) NOT NULL DEFAULT '',
  `tax` decimal(16,2) NOT NULL DEFAULT '',
  `tax2` decimal(16,2) NOT NULL DEFAULT '',
  `total` decimal(16,2) NOT NULL DEFAULT '',
  `taxrate` decimal(10,3) NOT NULL DEFAULT '',
  `taxrate2` decimal(10,3) NOT NULL DEFAULT '',
  `status` text DEFAULT NULL,
  `paymentmethod` text DEFAULT NULL,
  `notes` text DEFAULT NULL,
  PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Dumping data for table `tblinvoices`
--

LOCK TABLES `tblinvoices` WRITE;
/*!40000 ALTER TABLE `tblinvoices` DISABLE KEYS */;
INSERT INTO `tblinvoices` VALUES (1001,1,'@T@-1001','2024-01-14','2024-01-21','2024-01-15 08:12:44','0000-00-00 00:00:00','6.99','0.00','1.33','0.00','8.32','19.000','0.000','Paid','stripe',''),(1002,1,'@T@-1002','2024-02-14','2024-02-21','0000-00-00 00:00:00','0000-00-00 00:00:00','6.99','5.00','1.33','0.00','3.32','19.000','0.000','Unpaid','stripe',''),(1003,2,'','2024-03-02','2024-03-09','0000-00-00 00:00:00','0000-00-00 00:00:00','99.00','0.00','0.00','0.00','99.00','0.000','0.000','Cancelled','paypal','duplicate');
/*!40000 ALTER TABLE `tblinvoices` ENABLE KEYS */;
UNLOCK TABLES;

--
-- Table structure for table `tblinvoiceitems`
--

DROP TABLE IF EXISTS `tblinvoiceitems`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
CREATE TABLE `tblinvoiceitems` (
  `id` int(10) unsigned NOT NULL AUTO_INCREMENT,
  `invoiceid` int(10) NOT NULL DEFAULT '0',
  `userid` int(10) NOT NULL DEFAULT '0',
  `type` varchar(30) NOT NULL DEFAULT '',
  `relid` int(10) NOT NULL DEFAULT '0',
  `description` text DEFAULT NULL,
  `amount` decimal(16,2) NOT NULL DEFAULT '',
  `taxed` int(1) NOT NULL DEFAULT '0',
  `duedate` date DEFAULT NULL,
  `paymentmethod` text DEFAULT NULL,
  `notes` text DEFAULT NULL,
  PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Dumping data for table `tblinvoiceitems`
--

LOCK TABLES `tblinvoiceitems` WRITE;
/*!40000 ALTER TABLE `tblinvoiceitems` DISABLE KEYS */;
INSERT INTO `tblinvoiceitems` VALUES (1,1001,1,'Hosting',1,'Starter - ana-@T@.ro (14/01/2024 - 13/02/2024)','4.99',1,'2024-01-21','stripe',''),(2,1001,1,'Addon',1,'Daily Backups','2.00',1,'2024-01-21','stripe',''),(3,1002,1,'Hosting',1,'Starter - ana-@T@.ro (14/02/2024 - 13/03/2024)','4.99',1,'2024-02-21','stripe',''),(4,1002,1,'Addon',1,'Daily Backups','2.00',1,'2024-02-21','stripe',''),(5,1003,2,'Hosting',2,'Pro - obrien-@T@.ie','99.00',0,'2024-03-09','paypal','');
/*!40000 ALTER TABLE `tblinvoiceitems` ENABLE KEYS */;
UNLOCK TABLES;

--
-- Table structure for table `tblaccounts`
--

DROP TABLE IF EXISTS `tblaccounts`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
CREATE TABLE `tblaccounts` (
  `id` int(10) unsigned NOT NULL AUTO_INCREMENT,
  `userid` int(10) NOT NULL DEFAULT '0',
  `currency` int(10) NOT NULL DEFAULT '0',
  `gateway` text DEFAULT NULL,
  `date` datetime DEFAULT NULL,
  `description` text DEFAULT NULL,
  `amountin` decimal(16,2) NOT NULL DEFAULT '',
  `fees` decimal(16,2) NOT NULL DEFAULT '',
  `amountout` decimal(16,2) NOT NULL DEFAULT '',
  `rate` decimal(10,5) NOT NULL DEFAULT '',
  `transid` text DEFAULT NULL,
  `invoiceid` int(10) NOT NULL DEFAULT '0',
  `refundid` int(10) NOT NULL DEFAULT '0',
  PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Dumping data for table `tblaccounts`
--

LOCK TABLES `tblaccounts` WRITE;
/*!40000 ALTER TABLE `tblaccounts` DISABLE KEYS */;
INSERT INTO `tblaccounts` VALUES (1,1,0,'stripe','2024-01-15 08:12:44','Invoice Payment','8.32','0.54','0.00','1.00000','ch_@T@_1',1001,0),(2,1,0,'stripe','2024-01-20 10:00:00','Invoice Payment','0.00','0.00','2.00','1.00000','re_@T@_1',1001,1);
/*!40000 ALTER TABLE `tblaccounts` ENABLE KEYS */;
UNLOCK TABLES;

--
-- Table structure for table `tblcredit`
--

DROP TABLE IF EXISTS `tblcredit`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
CREATE TABLE `tblcredit` (
  `id` int(10) unsigned NOT NULL AUTO_INCREMENT,
  `clientid` int(10) NOT NULL DEFAULT '0',
  `date` date DEFAULT NULL,
  `description` text DEFAULT NULL,
  `amount` decimal(16,2) NOT NULL DEFAULT '',
  `relid` int(10) NOT NULL DEFAULT '0',
  PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Dumping data for table `tblcredit`
--

LOCK TABLES `tblcredit` WRITE;
/*!40000 ALTER TABLE `tblcredit` DISABLE KEYS */;
INSERT INTO `tblcredit` VALUES (1,1,'2023-12-01','Goodwill credit','30.00',0),(2,1,'2024-02-14','Credit Applied to Invoice #1002','-5.00',1002);
/*!40000 ALTER TABLE `tblcredit` ENABLE KEYS */;
UNLOCK TABLES;

--
-- Table structure for table `tblticketdepartments`
--

DROP TABLE IF EXISTS `tblticketdepartments`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
CREATE TABLE `tblticketdepartments` (
  `id` int(10) unsigned NOT NULL AUTO_INCREMENT,
  `name` text DEFAULT NULL,
  `description` text DEFAULT NULL,
  `email` text DEFAULT NULL,
  `clientsonly` text DEFAULT NULL,
  `piperepliesonly` text DEFAULT NULL,
  `noautoresponder` text DEFAULT NULL,
  `hidden` text DEFAULT NULL,
  `order` int(1) NOT NULL DEFAULT '0',
  PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Dumping data for table `tblticketdepartments`
--

LOCK TABLES `tblticketdepartments` WRITE;
/*!40000 ALTER TABLE `tblticketdepartments` DISABLE KEYS */;
INSERT INTO `tblticketdepartments` VALUES (1,'Support @T@','Technical help','support@example.net','','','','',1),(2,'Billing @T@','Invoices','billing@example.net','on','','','',2);
/*!40000 ALTER TABLE `tblticketdepartments` ENABLE KEYS */;
UNLOCK TABLES;

--
-- Table structure for table `tbltickets`
--

DROP TABLE IF EXISTS `tbltickets`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
CREATE TABLE `tbltickets` (
  `id` int(10) unsigned NOT NULL AUTO_INCREMENT,
  `tid` varchar(128) NOT NULL DEFAULT '',
  `did` int(10) NOT NULL DEFAULT '0',
  `userid` int(10) NOT NULL DEFAULT '0',
  `contactid` int(10) NOT NULL DEFAULT '0',
  `name` text DEFAULT NULL,
  `email` text DEFAULT NULL,
  `c` text DEFAULT NULL,
  `date` datetime DEFAULT NULL,
  `title` text DEFAULT NULL,
  `message` text DEFAULT NULL,
  `status` varchar(64) NOT NULL DEFAULT '',
  `urgency` text DEFAULT NULL,
  `admin` text DEFAULT NULL,
  `lastreply` datetime DEFAULT NULL,
  `flag` int(1) NOT NULL DEFAULT '0',
  PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Dumping data for table `tbltickets`
--

LOCK TABLES `tbltickets` WRITE;
/*!40000 ALTER TABLE `tbltickets` DISABLE KEYS */;
INSERT INTO `tbltickets` VALUES (1,'ABC-123456',1,1,0,'','','x1y2z3','2024-01-20 09:00:00','Site down @T@','My site returns 500 since this morning.\r\nPlease help!','Answered','High','','2024-01-20 09:30:00',0),(2,'DEF-654321',2,2,0,'','','q9w8e7','2024-03-03 12:00:00','Refund @T@','Please cancel invoice 1003.','Closed','Medium','','2024-03-03 13:00:00',0);
/*!40000 ALTER TABLE `tbltickets` ENABLE KEYS */;
UNLOCK TABLES;

--
-- Table structure for table `tblticketreplies`
--

DROP TABLE IF EXISTS `tblticketreplies`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
CREATE TABLE `tblticketreplies` (
  `id` int(10) unsigned NOT NULL AUTO_INCREMENT,
  `tid` int(10) NOT NULL DEFAULT '0',
  `userid` int(10) NOT NULL DEFAULT '0',
  `contactid` int(10) NOT NULL DEFAULT '0',
  `name` text DEFAULT NULL,
  `email` text DEFAULT NULL,
  `date` datetime DEFAULT NULL,
  `message` text DEFAULT NULL,
  `admin` text DEFAULT NULL,
  `attachment` text DEFAULT NULL,
  `rating` int(5) NOT NULL DEFAULT '0',
  PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Dumping data for table `tblticketreplies`
--

LOCK TABLES `tblticketreplies` WRITE;
/*!40000 ALTER TABLE `tblticketreplies` DISABLE KEYS */;
INSERT INTO `tblticketreplies` VALUES (3,2,0,0,'','','2024-03-03 13:00:00','Done, invoice cancelled.','Jane Admin','',0),(1,1,0,0,'','','2024-01-20 09:15:00','Looking into it now.','Jane Admin','',0),(2,1,1,0,'','','2024-01-20 09:30:00','It works again, thanks!','','',5);
/*!40000 ALTER TABLE `tblticketreplies` ENABLE KEYS */;
UNLOCK TABLES;

--
-- Table structure for table `tblannouncements`
--

DROP TABLE IF EXISTS `tblannouncements`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
CREATE TABLE `tblannouncements` (
  `id` int(10) unsigned NOT NULL AUTO_INCREMENT,
  `date` datetime DEFAULT NULL,
  `title` text DEFAULT NULL,
  `announcement` text DEFAULT NULL,
  `published` int(1) NOT NULL DEFAULT '0',
  `parentid` int(10) NOT NULL DEFAULT '0',
  `language` text DEFAULT NULL,
  PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Dumping data for table `tblannouncements`
--

LOCK TABLES `tblannouncements` WRITE;
/*!40000 ALTER TABLE `tblannouncements` DISABLE KEYS */;
INSERT INTO `tblannouncements` VALUES (1,'2024-01-01 00:00:00','Happy new year @T@','<p>Our <b>new</b> plans are live.</p>',1,0,''),(2,'2024-01-01 00:00:00','La multi ani','<p>…</p>',1,1,'romanian');
/*!40000 ALTER TABLE `tblannouncements` ENABLE KEYS */;
UNLOCK TABLES;

--
-- Table structure for table `tblknowledgebasecats`
--

DROP TABLE IF EXISTS `tblknowledgebasecats`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
CREATE TABLE `tblknowledgebasecats` (
  `id` int(10) unsigned NOT NULL AUTO_INCREMENT,
  `parentid` int(10) NOT NULL DEFAULT '0',
  `name` text DEFAULT NULL,
  `description` text DEFAULT NULL,
  `hidden` text DEFAULT NULL,
  `catid` int(10) NOT NULL DEFAULT '0',
  `language` text DEFAULT NULL,
  PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Dumping data for table `tblknowledgebasecats`
--

LOCK TABLES `tblknowledgebasecats` WRITE;
/*!40000 ALTER TABLE `tblknowledgebasecats` DISABLE KEYS */;
INSERT INTO `tblknowledgebasecats` VALUES (1,0,'Getting started @T@','Basics','',0,''),(2,0,'Primii pasi','','',1,'romanian');
/*!40000 ALTER TABLE `tblknowledgebasecats` ENABLE KEYS */;
UNLOCK TABLES;

--
-- Table structure for table `tblknowledgebase`
--

DROP TABLE IF EXISTS `tblknowledgebase`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
CREATE TABLE `tblknowledgebase` (
  `id` int(10) unsigned NOT NULL AUTO_INCREMENT,
  `title` text DEFAULT NULL,
  `article` text DEFAULT NULL,
  `views` int(10) NOT NULL DEFAULT '0',
  `votes` int(10) NOT NULL DEFAULT '0',
  `useful` int(10) NOT NULL DEFAULT '0',
  `order` int(3) NOT NULL DEFAULT '0',
  `parentid` int(10) NOT NULL DEFAULT '0',
  `language` text DEFAULT NULL,
  `private` text DEFAULT NULL,
  PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Dumping data for table `tblknowledgebase`
--

LOCK TABLES `tblknowledgebase` WRITE;
/*!40000 ALTER TABLE `tblknowledgebase` DISABLE KEYS */;
INSERT INTO `tblknowledgebase` VALUES (1,'How to upload files @T@','Use FTP or the file manager.',42,3,3,1,0,'',''),(2,'Cum incarc fisiere','Folositi FTP.',1,0,0,1,1,'romanian','');
/*!40000 ALTER TABLE `tblknowledgebase` ENABLE KEYS */;
UNLOCK TABLES;

--
-- Table structure for table `tblknowledgebaselinks`
--

DROP TABLE IF EXISTS `tblknowledgebaselinks`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
CREATE TABLE `tblknowledgebaselinks` (
  `id` int(10) unsigned NOT NULL AUTO_INCREMENT,
  `categoryid` int(10) NOT NULL DEFAULT '0',
  `articleid` int(10) NOT NULL DEFAULT '0',
  PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Dumping data for table `tblknowledgebaselinks`
--

LOCK TABLES `tblknowledgebaselinks` WRITE;
/*!40000 ALTER TABLE `tblknowledgebaselinks` DISABLE KEYS */;
INSERT INTO `tblknowledgebaselinks` VALUES (1,1,1);
/*!40000 ALTER TABLE `tblknowledgebaselinks` ENABLE KEYS */;
UNLOCK TABLES;

--
-- Table structure for table `tblpromotions`
--

DROP TABLE IF EXISTS `tblpromotions`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
CREATE TABLE `tblpromotions` (
  `id` int(10) unsigned NOT NULL AUTO_INCREMENT,
  `code` text DEFAULT NULL,
  `type` text DEFAULT NULL,
  `recurring` int(1) NOT NULL DEFAULT '0',
  `value` decimal(16,2) NOT NULL DEFAULT '',
  `cycles` text DEFAULT NULL,
  `appliesto` text DEFAULT NULL,
  `requires` text DEFAULT NULL,
  `requiresexisting` int(1) NOT NULL DEFAULT '0',
  `startdate` date DEFAULT NULL,
  `expirationdate` date DEFAULT NULL,
  `maxuses` int(10) NOT NULL DEFAULT '0',
  `uses` int(10) NOT NULL DEFAULT '0',
  `lifetimepromo` int(1) NOT NULL DEFAULT '0',
  `applyonce` int(1) NOT NULL DEFAULT '0',
  `newsignups` int(1) NOT NULL DEFAULT '0',
  `existingclient` int(1) NOT NULL DEFAULT '0',
  `onceperclient` int(1) NOT NULL DEFAULT '0',
  `recurfor` int(3) NOT NULL DEFAULT '0',
  `upgrades` int(1) NOT NULL DEFAULT '0',
  `upgradeconfig` text DEFAULT NULL,
  `notes` text DEFAULT NULL,
  PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Dumping data for table `tblpromotions`
--

LOCK TABLES `tblpromotions` WRITE;
/*!40000 ALTER TABLE `tblpromotions` DISABLE KEYS */;
INSERT INTO `tblpromotions` VALUES (1,'SPRING@T@','Percentage',1,'20.00','','1,2','',0,'2024-03-01','2024-05-31',100,7,0,0,1,0,0,0,0,'','Spring sale'),(2,'FIVEOFF@T@','Fixed Amount',0,'5.00','','','',0,'0000-00-00','0000-00-00',0,3,0,0,0,0,0,0,0,'',''),(3,'FREESETUP@T@','Free Setup',0,'0.00','','1','',0,'0000-00-00','0000-00-00',0,0,0,0,0,0,0,0,0,'',''),(4,'PRICE@T@','Price Override',0,'1.00','','1','',0,'0000-00-00','0000-00-00',0,0,0,0,0,0,0,0,0,'','');
/*!40000 ALTER TABLE `tblpromotions` ENABLE KEYS */;
UNLOCK TABLES;

--
-- Table structure for table `tbltax`
--

DROP TABLE IF EXISTS `tbltax`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
CREATE TABLE `tbltax` (
  `id` int(10) unsigned NOT NULL AUTO_INCREMENT,
  `level` int(1) NOT NULL DEFAULT '0',
  `name` text DEFAULT NULL,
  `state` text DEFAULT NULL,
  `country` text DEFAULT NULL,
  `taxrate` decimal(10,3) NOT NULL DEFAULT '',
  PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Dumping data for table `tbltax`
--

LOCK TABLES `tbltax` WRITE;
/*!40000 ALTER TABLE `tbltax` DISABLE KEYS */;
INSERT INTO `tbltax` VALUES (1,1,'VAT RO @T@','','RO','19.000'),(2,1,'MwSt AT @T@','','AT','20.000'),(3,2,'State tax @T@','CA','US','7.250');
/*!40000 ALTER TABLE `tbltax` ENABLE KEYS */;
UNLOCK TABLES;

/*!40014 SET UNIQUE_CHECKS=@OLD_UNIQUE_CHECKS */;
/*!40101 SET CHARACTER_SET_CLIENT=@OLD_CHARACTER_SET_CLIENT */;

-- Dump completed on 2024-03-15 12:00:00
