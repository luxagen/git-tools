#!/usr/bin/perl

use strict;
use warnings;

# Check required environment variables
my @missing;
for my $var (qw(GRM_CFGCMD GRM_RPATH_TEMPLATE GRM_RLOGIN)) {
    if (!defined $ENV{$var}) {
        push @missing, $var;
    }
}

if (@missing) {
    print STDERR "The following environment variables must be set: ", join(" ", @missing), "\n";
    exit 2;
}

# Determine whether this dir is already a git repo
my $virgin = !(-d ".git");

# Set path base
my $grm_rpath_base = '/git/music-projects';

# Get local and remote relative paths
my $lrel = `grm list-lrel`;
chomp($lrel);
my $rrel = `grm list-rrel`;
chomp($rrel);
my $grm_rpath = "$grm_rpath_base/$rrel";
$grm_rpath .= ".git" unless $grm_rpath =~ /\.git$/;

# There must be exactly one GRM-known (sub)directory and it must be .
if ($rrel eq "" || $ENV{PWD} !~ /$lrel$/) {
    print "The current directory is unknown to GRM!\n";
    exit 1;
}

print "About to create remote repo '$grm_rpath'; are you sure? ";
my $reply = <STDIN>;
chomp($reply);
if ($reply !~ /^[Yy]$/) {
    print "(aborted)\n";
    exit 0;
}

system("git init -q");
system($ENV{GRM_CFGCMD});

# Set error handling mode
$ENV{PERL_INLINE_C_FATAL_WARNINGS} = 1;  # Equivalent to set -e

# Create new remote repo based on remote template
system("ssh $ENV{GRM_RLOGIN} \"cp -na --reflink=auto '$grm_rpath_base/$ENV{GRM_RPATH_TEMPLATE}' '$grm_rpath'\"");

my $ssh_rpath = "ssh://$ENV{GRM_RLOGIN}$grm_rpath";

# Check if remote exists, add or update it accordingly
my $remote_exists = system("git remote get-url origin >/dev/null 2>&1") == 0;
if ($remote_exists) {
    # Remote exists, update it
    system("git remote set-url origin $ssh_rpath");
    system("git fetch origin");
} else {
    # Remote doesn't exist, add it
    system("git remote add -f origin $ssh_rpath");
}

if ($virgin) {
    system("git checkout master");
}